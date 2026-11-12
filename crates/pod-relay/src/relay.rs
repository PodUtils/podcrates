//! Which relay hosts podssh talks to, in what order, and the forward path for
//! a target.
//!
//! The default relay is compiled in so podssh works on a host with no setup
//! (podssh's operator's decision). `--relay-host` or the `PODSSH_RELAY`
//! environment variable replace it with an ordered list of hosts. With no list
//! given, the default host comes first and alternates from the relay's own
//! pool follow ([`crate::pool`]), so one unreachable host, or a proxy that
//! refuses one name, does not stop podssh. The default deployment, its host
//! and its limits are data of `pod-services` (the service `ajam-relay`).

/// The default deployment of the relay: the service `ajam-relay` of
/// `pod-services`, which holds its host, its limits and its seed.
pub fn deployment() -> &'static pod_services::Service {
    pod_services::get("ajam-relay").expect("pod-services holds the service ajam-relay")
}

/// A limit of the default deployment, a whole number.
fn limit(name: &str) -> u64 {
    let value = deployment().int(name).and_then(|n| u64::try_from(n).ok());
    value.unwrap_or_else(|| panic!("ajam-relay of pod-services holds no fact {name}"))
}

/// The relay podssh uses when nothing else is configured.
pub fn default_relay_host() -> &'static str {
    deployment().endpoint("default").map(|e| e.host()).expect("ajam-relay of pod-services has the endpoint default")
}

/// The relay closes a session after this many seconds with no traffic (its
/// `/relays.json`; the fact `idle-timeout-ms`), so keepalives must come
/// sooner.
pub fn relay_idle_secs() -> u64 {
    limit("idle-timeout-ms") / 1000
}

/// The relay's dial of a target, before the upgrade (the fact
/// `connect-timeout-ms`).
pub fn connect_timeout() -> std::time::Duration {
    std::time::Duration::from_millis(limit("connect-timeout-ms"))
}

/// The relay ends a session after this many payload bytes, both ways
/// together, with `1009 session byte cap` (the fact `max-session-bytes`).
pub fn session_byte_cap() -> u64 {
    limit("max-session-bytes")
}

/// The relay ends a session after this long (the fact `max-session-ms`).
pub fn session_time_cap() -> std::time::Duration {
    std::time::Duration::from_millis(limit("max-session-ms"))
}

/// podssh's own byte budget for one session, under the cap: the relay
/// counts bytes that podssh has not read yet, and can hold 2 MiB queued, so a
/// copy opens a new session first (podssh T-137). The IRC transfer keeps the same
/// margin for the same reason.
pub const SESSION_BYTE_BUDGET: u64 = 60 << 20;

/// podssh's time budget for one session, half an hour under the cap.
pub const SESSION_TIME_BUDGET: std::time::Duration = std::time::Duration::from_secs(11 * 3600 + 30 * 60);

/// The byte budget, with `env` (the consumer's word that lowers it, for a
/// relay with smaller caps or a test) lowering it: a whole number of bytes,
/// at least 1 MiB. Any other value is ignored. Under the limits of the relay
/// that the run of `ctx` uses ([`byte_budget_under`]).
pub fn byte_budget(ctx: &crate::context::Context, env: Option<&str>) -> u64 {
    byte_budget_under(env, &ctx.in_use())
}

/// The byte budget under a relay's `limits` (podssh T-342): the compiled-in one, or
/// less with the same margin when the relay's `max_session_bytes` is
/// smaller; a larger one never raises it. Then `env` lowers it.
pub fn byte_budget_under(env: Option<&str>, limits: &crate::pool::Limits) -> u64 {
    let margin = session_byte_cap() - SESSION_BYTE_BUDGET;
    let under = limits.max_session_bytes.and_then(|cap| cap.checked_sub(margin)).filter(|n| *n >= 1 << 20);
    let budget = under.map_or(SESSION_BYTE_BUDGET, |n| n.min(SESSION_BYTE_BUDGET));
    env.and_then(|v| v.trim().parse::<u64>().ok()).filter(|n| *n >= 1 << 20).map_or(budget, |n| n.min(budget))
}

/// The time budget under a relay's `limits` (podssh T-342): the compiled-in one, or
/// half an hour under a smaller `max_session_ms`.
pub fn time_budget(limits: &crate::pool::Limits) -> std::time::Duration {
    let margin = session_time_cap() - SESSION_TIME_BUDGET;
    let under = limits.max_session_ms.map(std::time::Duration::from_millis).and_then(|cap| cap.checked_sub(margin));
    under.filter(|d| !d.is_zero()).map_or(SESSION_TIME_BUDGET, |d| d.min(SESSION_TIME_BUDGET))
}

/// The relay's idle cut under its `limits` (podssh T-342): the compiled-in one, or
/// a smaller `idle_timeout_ms`.
pub fn idle_secs(limits: &crate::pool::Limits) -> u64 {
    let given = limits.idle_timeout_ms.map(|ms| ms / 1000).filter(|s| *s > 0);
    given.map_or(relay_idle_secs(), |s| s.min(relay_idle_secs()))
}

/// How many pool hosts follow the default host when no list is given. Each
/// failed host costs one bounded attempt; a handful covers a dead host or a
/// proxy that refuses one name without making a hopeless run slow.
pub const MAX_ALTERNATES: usize = 3;

/// A relay to connect to: TLS on `port` (443 unless stated), certificate
/// checked against `host`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Relay {
    /// A name in lower case, or an IP address: an IPv6 address with no
    /// brackets, in its canonical form (RFC 5952).
    pub host: String,
    pub port: u16,
}

impl Relay {
    /// `HOST`, or `HOST:PORT` when the port is not 443, with an IPv6 address
    /// in brackets: the relay's name in a line, in the cache and in the
    /// `Host` of a request. [`parse_relay`] reads it back.
    pub fn name(&self) -> String {
        pod_proxy::host::host_header(&self.host, self.port)
    }

    /// `https://` and [`Relay::name`].
    pub fn url(&self) -> String {
        format!("https://{}", self.name())
    }
}

/// `host` as a URL writes it: an IPv6 address in brackets.
pub fn url_host(host: &str) -> String {
    if host.contains(':') && !host.starts_with('[') { format!("[{host}]") } else { host.to_string() }
}

/// The hosts to try, in order. The first is also the key of the cached pool
/// of hosts; a token is cached for each deployment (`token::token_key`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelayList {
    pub hosts: Vec<Relay>,
    /// Whether the user named the hosts (then only those are tried).
    pub explicit: bool,
}

impl RelayList {
    /// The first host: the key of the cached pool.
    pub fn primary(&self) -> &Relay {
        &self.hosts[0]
    }
}

/// The hosts to use: the flag, else the consumer's variable (`env`), else the
/// default followed
/// by up to [`MAX_ALTERNATES`] of `pool` (hosts of the default relay's own pool,
/// best first).
pub fn select_relays(flag: Option<&str>, env: Option<String>, pool: &[String]) -> Result<RelayList, String> {
    let chosen = flag.map(str::to_string).or_else(|| env.filter(|v| !v.trim().is_empty()));
    if let Some(value) = chosen {
        let hosts = parse_relay_list(&value)?;
        return Ok(RelayList { hosts, explicit: true });
    }
    let mut hosts = vec![Relay { host: default_relay_host().to_string(), port: 443 }];
    for host in pool {
        if hosts.len() > MAX_ALTERNATES {
            break;
        }
        if let Ok(relay) = parse_relay(host)
            && !hosts.contains(&relay)
        {
            hosts.push(relay);
        }
    }
    Ok(RelayList { hosts, explicit: false })
}

/// The default list followed by `others`, the hosts of other deployments
/// that a signed list names (podssh T-346, [`crate::pool::kept::other_hosts`]),
/// each once; a list that the user named stays the relay alone.
pub fn with_others(mut list: RelayList, others: Vec<Relay>) -> RelayList {
    if list.explicit {
        return list;
    }
    for relay in others {
        if !list.hosts.contains(&relay) {
            list.hosts.push(relay);
        }
    }
    list
}

/// An ordered list: hosts separated by commas, each as [`parse_relay`]
/// accepts it. Commas only: splitting on spaces too would turn one mistyped
/// value (`"not a host"`) into a list of plausible hosts. Duplicates are
/// dropped; an empty list is an error.
pub fn parse_relay_list(value: &str) -> Result<Vec<Relay>, String> {
    let mut hosts: Vec<Relay> = Vec::new();
    for item in value.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        let relay = parse_relay(item)?;
        if !hosts.contains(&relay) {
            hosts.push(relay);
        }
    }
    if hosts.is_empty() {
        return Err(format!("no relay host in {value:?}"));
    }
    Ok(hosts)
}

/// Accepts `host`, `host:port`, `[ADDR]` and `[ADDR]:PORT` (an IPv6
/// address in brackets, as podnet's issue 273 asks), and the same with an
/// `https://` or `wss://` prefix and an optional trailing `/`, since that is
/// how the relay's own documents write it. The address is kept with no
/// brackets in its canonical form, so that each spelling gives one relay and
/// one name of its cache files. A bare IPv6 address is refused: its last
/// group and a port cannot be told apart. A zone (`%eth0`) is refused.
///
/// With the feature `plain-ws`, `http://` and `ws://` name a stand-in relay
/// on the loopback (port 80 unless stated), for the tests of an embedder,
/// which open its sessions with [`crate::open::open_on_loopback`]; a host
/// that is not the loopback is refused.
pub fn parse_relay(value: &str) -> Result<Relay, String> {
    let bad = |why: String| format!("bad relay {value:?}: {why}");
    let trimmed = value.trim();
    let (rest, plain) = match ["https://", "wss://"].iter().find_map(|s| trimmed.strip_prefix(s)) {
        Some(rest) => (rest, false),
        None => match plain(trimmed) {
            Some(rest) => (rest.map_err(bad)?, true),
            None => (trimmed, false),
        },
    };
    let (host, port) = host_and_port(rest.trim_end_matches('/'), if plain { 80 } else { 443 }).map_err(bad)?;
    // A token goes in clear on this wire: never to another host.
    if plain && !pod_proxy::env::is_loopback(&host) {
        return Err(bad("plain http:// and ws:// go only to a relay on the loopback".into()));
    }
    Ok(Relay { host, port })
}

/// The text after `http://` or `ws://`: a stand-in relay on the loopback
/// with the feature `plain-ws`, else refused; `None` for another scheme.
fn plain(text: &str) -> Option<Result<&str, String>> {
    let rest = ["http://", "ws://"].iter().find_map(|s| text.strip_prefix(s))?;
    if cfg!(feature = "plain-ws") {
        Some(Ok(rest))
    } else {
        Some(Err("a relay takes https:// or wss://; plain http:// and ws:// need the feature plain-ws".into()))
    }
}

/// `HOST`, `HOST:PORT`, `[ADDR]` or `[ADDR]:PORT`: the host in lower case,
/// an IPv6 address in its canonical form, and the port (`default` with
/// none).
fn host_and_port(text: &str, default: u16) -> Result<(String, u16), String> {
    let (host, port) = match text.strip_prefix('[') {
        Some(rest) => {
            let (inside, after) = rest.split_once(']').ok_or("'[' is not closed")?;
            let address = inside
                .parse::<std::net::Ipv6Addr>()
                .map_err(|_| format!("{inside:?} in brackets is no IPv6 address (a zone is not taken)"))?;
            let port = match after {
                "" => None,
                _ => Some(after.strip_prefix(':').ok_or_else(|| format!("{after:?} after ']'"))?),
            };
            (address.to_string(), port)
        }
        None => {
            let (host, port) = match text.rsplit_once(':') {
                Some((host, _)) if host.contains(':') => {
                    return Err("write an IPv6 address in brackets: [ADDR] or [ADDR]:PORT".into());
                }
                Some((host, port)) => (host, Some(port)),
                None => (text, None),
            };
            check_host(host)?;
            (host.to_ascii_lowercase(), port)
        }
    };
    let port = match port {
        None => default,
        Some(text) => text.parse::<u16>().ok().filter(|p| *p != 0).ok_or_else(|| format!("bad port {text:?}"))?,
    };
    Ok((host, port))
}

/// The relay's forward path for `host:port`: `/connect/<host>/<port>`.
///
/// The host is a DNS name, an IPv4 literal or a bare IPv6 literal (see
/// [`check_target`]), so nothing a user types can add a path segment, a
/// query string or a header to the request. The relay takes the bare IPv6
/// literal in the path (as measured), and `:` is valid in a path
/// segment (RFC 3986), where `[` and `]` are not.
pub fn forward_path(host: &str, port: u16) -> Result<String, String> {
    check_target(host)?;
    if port == 0 {
        return Err("port 0 is not a port".into());
    }
    Ok(format!("/connect/{host}/{port}"))
}

/// The path of a node's socket, `/v1/node/<name>`, for a name that cannot
/// change the path ([`check_node_name`]).
pub fn node_path(name: &str) -> Result<String, String> {
    check_node_name(name)?;
    Ok(format!("/v1/node/{name}"))
}

/// The path of an operator's socket, `/v1/connect/<name>`.
pub fn operator_path(name: &str) -> Result<String, String> {
    check_node_name(name)?;
    Ok(format!("/v1/connect/{name}"))
}

/// The checks of a host, a target and a pair name, defined once in
/// `podssh-ws` for each crate that builds a relay path.
pub use pod_core::names::{check_host, check_node_name, check_target};

/// Whether `host` is an IPv6 literal (with no brackets).
pub fn is_ipv6_literal(host: &str) -> bool {
    host.parse::<std::net::Ipv6Addr>().is_ok()
}

/// The words of podssh's relay role for an IPv6 target that its host cannot
/// route out (podssh T-253): its `502` before the upgrade, or its `1011` when a lazy
/// dial meets it after the upgrade.
pub const NO_IPV6_ROUTE: &str = "this relay's host has no IPv6 route out";

/// The default relay's reason for each session to an IPv6 target, as
/// measured while IPv4 to the same hosts worked: its egress reached none.
const CLOSED_AT_ONCE: &str = "target closed before sending anything";

/// Whether a relay's Close of a session to an IPv6 target says that the
/// relay reached no IPv6 host (podssh T-253): the default relay's `1011` with
/// `CLOSED_AT_ONCE`, or the role's own words.
pub fn unreached_ipv6(code: Option<u16>, reason: &str) -> bool {
    code == Some(1011) && (reason.contains(CLOSED_AT_ONCE) || reason.contains(NO_IPV6_ROUTE))
}

/// A note for a session to an IPv6 target that the relay closed with
/// `reason`, the default relay's or the role's: the roads that carry IPv6.
pub fn ipv6_note(ipv6_target: bool, reason: &str) -> Option<&'static str> {
    (ipv6_target && (reason.contains(CLOSED_AT_ONCE) || reason.contains(NO_IPV6_ROUTE))).then_some(
        "an IPv6 target that closes at once usually means that the relay has no IPv6 route out: a relay of \
         yours on a host with IPv6 (podssh relay serve, then --relay-host), a node with IPv6 egress (-J \
         node://NAME) or warp://[ADDR] can carry it",
    )
}

#[cfg(test)]
#[path = "relay_tests.rs"]
mod tests;
