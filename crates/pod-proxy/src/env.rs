//! The proxy that the variables of an environment name: `https_proxy`,
//! `HTTPS_PROXY`, `all_proxy`, `ALL_PROXY`, minus what `no_proxy` or
//! `NO_PROXY` excludes, and minus the loopback; or, when no variable names
//! one, the proxy of a tool's configuration ([`crate::detect`]); or a value
//! that the user gave the consumer on purpose ([`Environment::with_proxy`]).
//!
//! The crate reads no variable and no file of its own:
//! [`Environment::read_with`] reads the variables through the reader that
//! the caller gives, only when the caller asks, and keeps where each value
//! came from ([`Origin`]). The [`Rules`] hold what the consumers read in
//! their own ways: the order of the variables, a URL that cannot be used,
//! the port of an `http://` URL, the ports of `NO_PROXY`, the loopback.
//!
//! On the hosts that the consumers are made for, the proxy is often the
//! only way out: a direct `connect()` is refused or dropped and DNS does not
//! resolve, while the proxy accepts `CONNECT name:443` and resolves the name
//! itself. So an HTTP proxy and a `socks5h://` proxy get the target's
//! *name*, never an address.

mod no_proxy;

use std::ffi::OsString;
use std::fmt;
use std::net::IpAddr;
use std::path::{Path, PathBuf};

pub use no_proxy::{NoProxy, is_loopback, no_proxy_entry, no_proxy_entry_at, no_proxy_matches};

use crate::proxy::Proxy;

/// The variables that name a proxy, in the order that they are read: the
/// first that is set, and not empty, is the proxy. Lower case first, as
/// curl reads `https_proxy`.
pub const PROXY_VARS: [&str; 4] = ["https_proxy", "HTTPS_PROXY", "all_proxy", "ALL_PROXY"];

/// The variables that list the hosts that bypass the proxy, in the order
/// that they are read.
pub const NO_PROXY_VARS: [&str; 2] = ["no_proxy", "NO_PROXY"];

/// [`PROXY_VARS`] with the upper case first, for a consumer that reads them
/// so (podnet).
pub const PROXY_VARS_UPPER_FIRST: [&str; 4] = ["HTTPS_PROXY", "https_proxy", "ALL_PROXY", "all_proxy"];

/// [`NO_PROXY_VARS`] with the upper case first.
pub const NO_PROXY_VARS_UPPER_FIRST: [&str; 2] = ["NO_PROXY", "no_proxy"];

/// Where a value of the proxy settings came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Origin {
    /// A variable of the environment that other programs read too
    /// (`https_proxy`, `NO_PROXY`), by its name.
    Variable(String),
    /// A value that the user gave to the consumer on purpose: an option or
    /// a variable of its own (`--proxy`), by the name that the consumer
    /// gives it. Only such a value can name a proxy command (`+exec:`).
    User(String),
    /// The configuration file of a tool (`~/.gitconfig`, `/etc/environment`)
    /// that names a proxy ([`crate::detect`]).
    File(PathBuf),
}

impl fmt::Display for Origin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Origin::Variable(name) | Origin::User(name) => f.write_str(name),
            Origin::File(path) => write!(f, "{}", path.display()),
        }
    }
}

/// A value of the proxy settings, and where it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Given<T> {
    pub value: T,
    pub origin: Origin,
}

/// How a consumer reads its proxy settings. The default is podssh's; the
/// README gives podnet's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rules {
    /// The variables of the proxy, in the order that they are read.
    pub proxy_vars: &'static [&'static str],
    /// The variables of the `NO_PROXY` list, in the order that they are read.
    pub no_proxy_vars: &'static [&'static str],
    /// A variable whose URL cannot be used is passed over, kept in
    /// [`Environment::skipped`], and the next one is read. Else it is the
    /// proxy, and an error for each target that takes it.
    pub skip_unusable: bool,
    /// The port of an `http://` URL with no port ([`crate::proxy::HTTP_PORT`]).
    pub http_port: u16,
    /// An entry of `NO_PROXY` with a port (`internal:8443`) excludes that
    /// port only. Else the port of an entry is passed over.
    pub no_proxy_ports: bool,
    /// A loopback target, and a name that the caller pinned to loopback
    /// addresses alone, goes directly: the proxy's loopback is not this
    /// host's. Else such a target takes the proxy unless `NO_PROXY` names it.
    pub loopback_direct: bool,
}

impl Default for Rules {
    fn default() -> Rules {
        Rules {
            proxy_vars: &PROXY_VARS,
            no_proxy_vars: &NO_PROXY_VARS,
            skip_unusable: false,
            http_port: crate::proxy::HTTP_PORT,
            no_proxy_ports: false,
            loopback_direct: true,
        }
    }
}

impl Rules {
    /// The proxy of `spec`, which came from `origin`: a command (`+exec:`)
    /// only from [`Origin::User`].
    pub fn parse(&self, spec: &str, origin: &Origin) -> Result<Proxy, String> {
        Proxy::parse_with(spec, matches!(origin, Origin::User(_)), self.http_port)
    }
}

/// The proxy settings of an environment, as the caller's reader gave them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Environment {
    /// The proxy of the first variable of the rules that is set and not
    /// empty, or why its URL cannot be used; or the proxy that the user gave
    /// or that a tool's file names.
    pub proxy: Option<Given<Result<Proxy, String>>>,
    /// The list of the first variable of the rules that is set and not
    /// empty.
    pub no_proxy: Option<Given<String>>,
    /// The values that were passed over, each with why: a URL that cannot
    /// be used ([`Rules::skip_unusable`]), and the proxy of a tool's file
    /// that cannot be used.
    pub skipped: Vec<Given<String>>,
    pub rules: Rules,
}

/// Why a target takes a proxy or goes directly: the outcome of the decision
/// `egress` of an observer ([`pod_core::Observer`]).
pub(crate) type Decision = (Result<Option<Proxy>, String>, &'static str);

impl Environment {
    /// Read [`PROXY_VARS`] and [`NO_PROXY_VARS`] through `var` with the
    /// default [`Rules`]: a consumer passes `&|name| std::env::var_os(name)`.
    /// Lower-case names win over upper-case ones, as in curl; an empty
    /// value, and one that is not UTF-8, counts as unset.
    pub fn read(var: &dyn Fn(&str) -> Option<OsString>) -> Environment {
        Environment::read_with(var, Rules::default())
    }

    /// Read the variables of `rules` through `var`.
    pub fn read_with(var: &dyn Fn(&str) -> Option<OsString>, rules: Rules) -> Environment {
        let set = |name: &str| var(name)?.into_string().ok().filter(|v| !v.trim().is_empty());
        let mut env = Environment { rules, ..Environment::default() };
        for name in env.rules.proxy_vars {
            let Some(value) = set(name) else { continue };
            let origin = Origin::Variable(name.to_string());
            match env.rules.parse(&value, &origin) {
                Err(why) if env.rules.skip_unusable => {
                    tracing::warn!(variable = %name, error = %why, "a proxy variable that cannot be used is passed over");
                    env.skipped.push(Given { value: why, origin });
                }
                value => {
                    env.proxy = Some(Given { value, origin });
                    break;
                }
            }
        }
        env.no_proxy = env
            .rules
            .no_proxy_vars
            .iter()
            .find_map(|name| set(name).map(|value| Given { value, origin: Origin::Variable(name.to_string()) }));
        if let Some(list) = &env.no_proxy {
            for entry in no_proxy::unreadable(&list.value) {
                tracing::warn!(%entry, variable = %list.origin, "a NO_PROXY entry that cannot be read matches nothing");
            }
        }
        env
    }

    /// This environment with the proxy that the user gave on purpose,
    /// `spec`, from `origin` (`Origin::User("--proxy")`), in place of the
    /// proxy of the variables: it can be a command, and the `NO_PROXY` list
    /// still applies. A URL that cannot be used is kept with why.
    pub fn with_proxy(mut self, spec: &str, origin: Origin) -> Environment {
        let value = self.rules.parse(spec, &origin);
        self.proxy = Some(Given { value, origin });
        self
    }

    /// When no variable names a proxy: the proxy of the first tool's file
    /// that names one ([`crate::detect::detect`]), with the file as its
    /// origin. `var` gives `HOME`; `read` reads a file. A proxy there that
    /// cannot be used is passed over, into [`Environment::skipped`]; a file
    /// never names a command.
    pub fn or_detected(
        mut self,
        var: &dyn Fn(&str) -> Option<OsString>,
        read: &dyn Fn(&Path) -> Option<String>,
    ) -> Environment {
        if self.proxy.is_some() {
            return self;
        }
        let home = var("HOME").filter(|h| !h.is_empty()).map(PathBuf::from);
        let Some((spec, path)) = crate::detect::detect(home.as_deref(), read) else { return self };
        let origin = Origin::File(path);
        match self.rules.parse(&spec, &origin) {
            Ok(proxy) => {
                tracing::info!(proxy = %proxy.label(), from = %origin, "no proxy variable; the proxy of a tool's file is used");
                self.proxy = Some(Given { value: Ok(proxy), origin });
            }
            Err(why) => {
                tracing::warn!(from = %origin, error = %why, "a tool's file names a proxy that cannot be used");
                self.skipped.push(Given { value: why, origin });
            }
        }
        self
    }

    /// The proxy for `target_host`, or none, with no port known: the port
    /// of a `NO_PROXY` entry is passed over. See [`Environment::proxy_at`].
    pub fn proxy_for(&self, target_host: &str, pinned: &[IpAddr]) -> Result<Option<Proxy>, String> {
        self.decide(target_host, None, pinned).0
    }

    /// The proxy for `target_host:port`, or none. With
    /// [`Rules::loopback_direct`], a loopback target never goes through a
    /// proxy (as in Go's net/http), as the proxy's loopback is not this
    /// host's, and podvm measured a proxy that answered 405 to `CONNECT
    /// 127.0.0.1`; a name that the caller pinned to loopback addresses alone
    /// (`pinned`, the pins of `target_host`) is such a target too, and a pin
    /// to any other address keeps the proxy, which is given the name. Then
    /// what the `NO_PROXY` list excludes. `Err` when the URL of the proxy
    /// cannot be used, with why.
    pub fn proxy_at(&self, target_host: &str, port: u16, pinned: &[IpAddr]) -> Result<Option<Proxy>, String> {
        self.decide(target_host, Some(port), pinned).0
    }

    /// [`Environment::proxy_at`], and the outcome for an observer: `none`
    /// (no proxy is set), `loopback`, `no_proxy`, `unusable`, or the word of
    /// the proxy ([`Proxy::outcome`]).
    pub(crate) fn decide(&self, target_host: &str, port: Option<u16>, pinned: &[IpAddr]) -> Decision {
        let Some(proxy) = &self.proxy else { return (Ok(None), "none") };
        if self.rules.loopback_direct
            && (is_loopback(target_host) || (!pinned.is_empty() && pinned.iter().all(IpAddr::is_loopback)))
        {
            return (Ok(None), "loopback");
        }
        if self.no_proxy_entry_at(target_host, port).is_some() {
            return (Ok(None), "no_proxy");
        }
        match &proxy.value {
            Ok(p) => (Ok(Some(p.clone())), p.outcome()),
            Err(why) => (Err(why.clone()), "unusable"),
        }
    }

    /// The first entry of the `NO_PROXY` list that excludes `host`
    /// ([`no_proxy_entry`]).
    pub fn no_proxy_entry(&self, host: &str) -> Option<&str> {
        self.no_proxy_entry_at(host, None)
    }

    /// The first entry of the `NO_PROXY` list that excludes `host:port`, by
    /// the ports of the rules ([`no_proxy_entry_at`]).
    pub fn no_proxy_entry_at(&self, host: &str, port: Option<u16>) -> Option<&str> {
        let port = port.filter(|_| self.rules.no_proxy_ports);
        self.no_proxy.as_ref().and_then(|list| no_proxy_entry_at(&list.value, host, port))
    }
}

/// The proxy that the variables of `var` select for `target_host`:
/// [`Environment::read`], then [`Environment::proxy_for`] with no pin.
pub fn proxy_from_vars(target_host: &str, var: &dyn Fn(&str) -> Option<OsString>) -> Result<Option<Proxy>, String> {
    Environment::read(var).proxy_for(target_host, &[])
}

#[cfg(test)]
#[path = "env_tests.rs"]
mod tests;
