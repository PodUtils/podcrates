//! The rules of a WARP proxy (PodUtils/podnet issues 132 and 136): for each
//! destination, go through the tunnel, connect directly from the host, or
//! refuse.
//!
//! A rule is `KIND:VALUE=ACTION`. KIND is `domain` (`NAME`, or `.NAME` for
//! the name and each name under it) or `cidr` (`ADDR/PREFIX`, IPv4 or IPv6,
//! or `@FILE` with one prefix on each line). ACTION is `tunnel`, `direct`
//! or `reject`. The first rule that matches wins. A name matches the domain
//! rules first; when no domain rule matches and there is a cidr rule, the
//! name is resolved through the tunnel and its addresses match the cidr
//! rules. With no match, the built-in rules apply (a loopback, a link-local
//! or an unspecified address, and `localhost`, are refused: WARP does not
//! reach them), then the default action.
//!
//! The lint of shadowed rules is the caller's ([`Lint`]): podnet passes the
//! lint of its netns rules (`podnet_netns::rules::shadow::check`,
//! PodUtils/podnet#452), so that `warp proxy -r` and `--rules` refuse a
//! shadowed `reject` as the netns rules do (PodUtils/podnet@T-242).

use std::net::IpAddr;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use std::sync::{Arc, RwLock};

use pod_core::{Class, Failure, Stage};

pub mod matcher;

pub use matcher::{Net, RuleMatch};

fn usage(text: String) -> Failure {
    Failure::new(Stage::Warp, Class::Usage, text)
}

/// What to do with a destination.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// Through the WARP session (the default).
    Tunnel,
    /// From the host, with no proxy and the resolver of the host.
    Direct,
    /// Refuse: SOCKS5 reply 0x02, HTTP 403.
    Reject,
}

impl Action {
    pub fn parse(text: &str) -> Result<Action, Failure> {
        match text.trim().to_ascii_lowercase().as_str() {
            "tunnel" | "warp" => Ok(Action::Tunnel),
            "direct" => Ok(Action::Direct),
            "reject" | "block" => Ok(Action::Reject),
            other => Err(usage(format!("{other:?} is not an action (tunnel, direct, reject)"))),
        }
    }

    /// The normal form of the action: `reject` for `reject` and `block`.
    pub fn name(self) -> &'static str {
        match self {
            Action::Tunnel => "tunnel",
            Action::Direct => "direct",
            Action::Reject => "reject",
        }
    }
}

/// One rule.
#[derive(Clone, Debug)]
pub struct Rule {
    pub matcher: RuleMatch,
    pub action: Action,
    /// The text of the rule, for the log.
    pub spec: String,
    /// Where the rule is: `-r 2`, `FILE:7`.
    pub place: String,
}

impl Rule {
    /// `KIND:VALUE=ACTION`, at `place`.
    pub fn parse(spec: &str, place: impl Into<String>) -> Result<Rule, Failure> {
        let spec = spec.trim();
        let (matcher, action) =
            spec.rsplit_once('=').ok_or_else(|| usage(format!("{spec:?}: give KIND:VALUE=ACTION")))?;
        let matcher = RuleMatch::parse(matcher).map_err(|e| usage(format!("{spec:?}: {e}")))?;
        let action = Action::parse(action).map_err(|e| usage(format!("{spec:?}: {e}")))?;
        Ok(Rule { matcher, action, spec: spec.to_string(), place: place.into() })
    }

    /// The text of the matcher: `KIND:VALUE`.
    pub fn matcher_text(&self) -> &str {
        self.spec.rsplit_once('=').map_or(self.spec.as_str(), |(m, _)| m)
    }
}

/// One rule as the lint of the caller reads it.
#[derive(Clone, Copy, Debug)]
pub struct LintItem<'a> {
    /// The matcher, `KIND:VALUE`, as podnet's `RuleMatch::parse` reads it.
    pub matcher: &'a str,
    /// The action in its normal form ([`Action::name`]).
    pub action: &'a str,
    /// Where the rule is.
    pub place: &'a str,
    /// The text of the rule.
    pub spec: &'a str,
}

/// The lint of shadowed rules, of the caller: one warning for each shadowed
/// rule, or the refusal of a shadowed `reject` when `allow` is false. A
/// closure of this form is a lint.
pub trait Lint: Send + Sync {
    fn check(&self, items: &[LintItem<'_>], allow: bool) -> Result<Vec<String>, String>;
}

impl<F> Lint for F
where
    F: Fn(&[LintItem<'_>], bool) -> Result<Vec<String>, String> + Send + Sync,
{
    fn check(&self, items: &[LintItem<'_>], allow: bool) -> Result<Vec<String>, String> {
        self(items, allow)
    }
}

/// No lint: each table passes.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoLint;

impl Lint for NoLint {
    fn check(&self, _items: &[LintItem<'_>], _allow: bool) -> Result<Vec<String>, String> {
        Ok(Vec::new())
    }
}

/// The rules of the proxy, in order, and the default action.
#[derive(Clone, Debug)]
pub struct Table {
    pub rules: Vec<Rule>,
    pub default: Action,
}

impl Default for Table {
    fn default() -> Table {
        Table { rules: Vec::new(), default: Action::Tunnel }
    }
}

/// A destination: an address, or a name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Host {
    Ip(IpAddr),
    Domain(String),
}

impl Table {
    /// The rules of the flags (`-r 1`, `-r 2`, ...), then the rules of
    /// `file`; then the caller's lint over them. A shadowed `reject` is a
    /// usage failure (exit 64) unless `allow`; the answer has the warnings.
    pub fn build(
        specs: &[String],
        file: Option<&Path>,
        default: Action,
        lint: &dyn Lint,
        allow: bool,
    ) -> Result<(Table, Vec<String>), Failure> {
        let mut rules = Vec::new();
        for spec in specs.iter().flat_map(|s| s.split(',')).filter(|s| !s.trim().is_empty()) {
            let place = format!("-r {}", rules.len() + 1);
            rules.push(Rule::parse(spec, place)?);
        }
        if let Some(file) = file {
            rules.extend(load_file(file)?);
        }
        let items: Vec<LintItem<'_>> = rules
            .iter()
            .map(|r| LintItem { matcher: r.matcher_text(), action: r.action.name(), place: &r.place, spec: &r.spec })
            .collect();
        let warnings = lint.check(&items, allow).map_err(usage)?;
        Ok((Table { rules, default }, warnings))
    }

    /// True when a rule matches addresses.
    pub fn has_cidr(&self) -> bool {
        self.rules.iter().any(|r| matches!(r.matcher, RuleMatch::Cidr(_)))
    }

    /// The action of a name, by the domain rules; `None` when no domain
    /// rule matches and the name is not a built-in one.
    pub fn for_name(&self, name: &str) -> Option<Action> {
        if let Some(rule) = self.rules.iter().find(|r| r.matcher.matches(name, None)) {
            return Some(rule.action);
        }
        let name = name.trim_end_matches('.').to_ascii_lowercase();
        (name == "localhost" || name.ends_with(".localhost")).then_some(Action::Reject)
    }

    /// The action of an address: the rules, the built-in rules, the
    /// default.
    pub fn for_ip(&self, ip: IpAddr) -> Action {
        self.rule_for_ip(ip).or_else(|| builtin(ip)).unwrap_or(self.default)
    }

    fn rule_for_ip(&self, ip: IpAddr) -> Option<Action> {
        let text = ip.to_string();
        self.rules.iter().find(|r| r.matcher.matches(&text, Some(ip))).map(|r| r.action)
    }

    /// The action of the addresses of a name: the first address that a rule
    /// or a built-in rule matches, else the default.
    pub fn for_addresses(&self, addresses: &[IpAddr]) -> Action {
        addresses.iter().find_map(|ip| self.rule_for_ip(*ip).or_else(|| builtin(*ip))).unwrap_or(self.default)
    }

    /// The action for `host` with no lookup of a name (a proxy with no
    /// resolver in the tunnel, PodUtils/podnet issue 140). A name that only
    /// an address rule can decide gets the default.
    pub fn decide_static(&self, host: &Host) -> Action {
        match host {
            Host::Ip(ip) => self.for_ip(*ip),
            Host::Domain(name) => self.for_name(name).unwrap_or(self.default),
        }
    }

    /// The action for `host`, with the addresses that `names` gave to
    /// decide it (for the tunnel to use them). A name is resolved only when
    /// no domain rule decides it and an address rule can.
    pub async fn decide(&self, names: &dyn Lookup, host: &Host) -> Result<(Action, Option<Vec<IpAddr>>), Failure> {
        Ok(match host {
            Host::Ip(ip) => (self.for_ip(*ip), None),
            Host::Domain(name) => match self.for_name(name) {
                Some(action) => (action, None),
                None if self.has_cidr() => {
                    let addresses = names.lookup(name).await?;
                    (self.for_addresses(&addresses), Some(addresses))
                }
                None => (self.default, None),
            },
        })
    }
}

/// The addresses of a name inside the tunnel, for the address rules of
/// [`Table::decide`]: the names of a proxy ([`crate::proxy::Names`] over a
/// dial), or the DNS over TCP of a [`crate::WarpNet`].
pub trait Lookup: Send + Sync {
    fn lookup<'a>(&'a self, name: &'a str) -> pod_core::BoxFuture<'a, Result<Vec<IpAddr>, Failure>>;
}

impl Lookup for crate::net::WarpNet {
    fn lookup<'a>(&'a self, name: &'a str) -> pod_core::BoxFuture<'a, Result<Vec<IpAddr>, Failure>> {
        Box::pin(crate::dns::lookup(self, &crate::dns::RESOLVERS, name, crate::net::DNS_TIMEOUT))
    }
}

impl Host {
    /// The host of a target: its address, or its name.
    pub fn of(target: &pod_core::Target) -> Host {
        match target {
            pod_core::Target::Addr(addr) => Host::Ip(addr.ip()),
            pod_core::Target::Name { host, .. } => Host::Domain(host.clone()),
        }
    }
}

impl std::fmt::Display for Host {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Host::Ip(ip) => write!(f, "{ip}"),
            Host::Domain(name) => f.write_str(name),
        }
    }
}

/// The built-in rules: WARP does not reach these addresses.
fn builtin(ip: IpAddr) -> Option<Action> {
    let ip = ip.to_canonical();
    let local = ip.is_loopback()
        || ip.is_unspecified()
        || match ip {
            IpAddr::V4(v4) => v4.is_link_local(),
            IpAddr::V6(v6) => (v6.segments()[0] & 0xffc0) == 0xfe80,
        };
    local.then_some(Action::Reject)
}

/// The rules of a file: one `KIND:VALUE=ACTION` on each line; a blank line,
/// and the text after `#`, are ignored. A bad line is a failure with its
/// number.
pub fn load_file(path: &Path) -> Result<Vec<Rule>, Failure> {
    let text = std::fs::read_to_string(path).map_err(|e| {
        let text = format!("reading the rules file {}", path.display());
        Failure::caused(Stage::Warp, Class::Usage, text, e)
    })?;
    let mut rules = Vec::new();
    for (n, line) in text.lines().enumerate() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let place = format!("{}:{}", path.display(), n + 1);
        rules.push(Rule::parse(line, place.clone()).map_err(|e| usage(format!("{place}: {e}")))?);
    }
    Ok(rules)
}

/// The counters of the decisions of a proxy.
#[derive(Debug, Default)]
pub struct Counters {
    pub tunnel: AtomicU64,
    pub direct: AtomicU64,
    pub reject: AtomicU64,
    pub reloads: AtomicU64,
}

impl Counters {
    pub fn count(&self, action: Action) {
        match action {
            Action::Tunnel => &self.tunnel,
            Action::Direct => &self.direct,
            Action::Reject => &self.reject,
        }
        .fetch_add(1, Relaxed);
    }
}

/// The rules of the proxies of a caller, which a reload of the rules file
/// replaces, and their counters. A value of the caller: no global.
#[derive(Debug, Default)]
pub struct Rules {
    table: RwLock<Arc<Table>>,
    pub counters: Counters,
}

impl Rules {
    pub fn new(table: Table) -> Rules {
        Rules { table: RwLock::new(Arc::new(table)), counters: Counters::default() }
    }

    /// Use `table` from now on.
    pub fn set(&self, table: Table) {
        *self.table.write().unwrap_or_else(|e| e.into_inner()) = Arc::new(table);
        self.counters.reloads.fetch_add(1, Relaxed);
    }

    /// The rules of now.
    pub fn current(&self) -> Arc<Table> {
        self.table.read().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

/// The error of a destination that a rule refuses.
#[derive(Debug)]
pub struct Refused(pub String);

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} is not allowed by the rules of the proxy", self.0)
    }
}

impl std::error::Error for Refused {}

impl Refused {
    /// The failure of the refusal: the class `Policy`.
    pub fn failure(self) -> Failure {
        Failure::caused(Stage::Warp, Class::Policy, self.to_string(), self)
    }
}

#[cfg(test)]
mod tests;
