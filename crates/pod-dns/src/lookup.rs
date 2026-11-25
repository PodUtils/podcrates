//! The addresses of a name from the resolvers in turn: the first resolver
//! that answers with addresses wins. An answer that the name does not exist
//! ends the lookup ([`Nxdomain::End`]), or passes over the other addresses
//! of its operator ([`Nxdomain::NextOperator`]).

use std::future::Future;
use std::net::IpAddr;

use pod_proxy::Missed;
use tokio::time::Instant;

use crate::NXDOMAIN;
use crate::doh::{Doh, Form};
use crate::line::Phrase;
use crate::settings::Nxdomain;

/// The lookup over the hosts of `order` (each the index of its resolver in
/// `resolvers`, and its address or its name), with `ask` asking one of them.
/// The failure keeps its parts for the line of the dialer ([`fold`]).
pub(crate) async fn over<F, Fut>(
    resolvers: &[Doh],
    order: Vec<(usize, String)>,
    rule: Nxdomain,
    host: &str,
    deadline: Instant,
    ask: F,
) -> Result<Vec<IpAddr>, Phrase>
where
    F: Fn(usize, String) -> Fut,
    Fut: Future<Output = Result<(Vec<IpAddr>, Form), Missed>>,
{
    let mut reasons = Vec::new();
    // The operators that said that the name does not exist.
    let mut said: Vec<&str> = Vec::new();
    for (i, at) in order {
        let operator = resolvers[i].operator.as_str();
        if said.contains(&operator) {
            continue;
        }
        if Instant::now() >= deadline {
            reasons.push((String::new(), "out of time".to_string()));
            break;
        }
        match ask(i, at.clone()).await {
            Ok((ips, _)) => return Ok(ips),
            Err(e) if e.why.ends_with(NXDOMAIN) && rule == Nxdomain::End => return Err(e.why.into()),
            Err(e) if e.why.ends_with(NXDOMAIN) => {
                said.push(operator);
                reasons.push((at, e.why));
            }
            Err(e) => reasons.push((at, e.why)),
        }
    }
    if !said.is_empty() {
        let verb = if said.len() > 1 { "say" } else { "says" };
        return Err(format!("{} {verb} that {host} does not exist", said.join(" and ")).into());
    }
    if reasons.is_empty() {
        return Err("no resolver answered".into());
    }
    tracing::debug!(%host, failures = %each(&reasons), "each resolver of DNS over HTTPS failed");
    Err(fold(&reasons))
}

/// The failures of the resolvers, each `(address, why)`, in groups of the
/// same cause, in order. The address of a resolver in its own words is
/// taken out first, so that a failure that names it reads alike.
fn groups(reasons: &[(String, String)]) -> Vec<(String, Vec<&str>)> {
    let mut groups: Vec<(String, Vec<&str>)> = Vec::new();
    for (at, why) in reasons {
        let cause = if at.is_empty() {
            why.clone()
        } else {
            let rest = why.strip_prefix(at.as_str()).map_or(why.as_str(), |r| r.trim_start_matches(':').trim_start());
            without_address(rest, at)
        };
        match groups.iter_mut().find(|g| g.0 == cause) {
            Some(g) => g.1.push(at),
            None => groups.push((cause, vec![at])),
        }
    }
    groups
}

/// `cause` with no address `at`: `could not connect to [A]:443: X` is
/// `could not connect: X`, and another place of the address is "the
/// address of the resolver".
fn without_address(cause: &str, at: &str) -> String {
    let mut out = cause.to_string();
    for name in [format!("[{at}]"), at.to_string()] {
        while let Some(i) = out.find(&name) {
            let end = i + name.len();
            let digits = out[end..]
                .strip_prefix(':')
                .map_or(0, |r| r.len() - r.trim_start_matches(|c: char| c.is_ascii_digit()).len());
            if digits > 0 {
                let start = if out[..i].ends_with(" to ") { i - 4 } else { i };
                out.replace_range(start..end + 1 + digits, "");
            } else {
                out.replace_range(i..end, "the address of the resolver");
            }
        }
    }
    out
}

/// `text` with no `(os error N)` of the system: the kind says it in words.
fn without_os_error(text: &str) -> String {
    match text.find(" (os error ") {
        Some(i) => match text[i..].find(')') {
            Some(end) => format!("{}{}", &text[..i], without_os_error(&text[i + end + 1..])),
            None => text.to_string(),
        },
        None => text.to_string(),
    }
}

/// The full list of the failures of the resolvers, for a log at debug: a
/// failure that each resolver had in the same words is said once with the
/// count, and a resolver is named alone only where its failure differs
/// (PodUtils/podssh#313).
fn each(reasons: &[(String, String)]) -> String {
    let groups = groups(reasons);
    if let [(cause, ats)] = &groups[..]
        && ats.len() > 1
    {
        return format!("each of {} resolvers failed: {cause}", ats.len());
    }
    let one = |(cause, ats): &(String, Vec<&str>)| match &ats[..] {
        [""] => cause.clone(),
        [at] => format!("{at}: {cause}"),
        _ => format!("{} resolvers ({}): {cause}", ats.len(), ats.join(", ")),
    };
    groups.iter().map(one).collect::<Vec<_>>().join("; ")
}

/// One phrase of the failures of the resolvers, each `(address, why)`, for
/// a line of an error (PodUtils/podnet#698): the count, and each cause
/// once with the count of its resolvers, the largest group first
/// (PodUtils/podnet#753). A cause has no address and no number of the
/// system. The full list is [`each`].
pub(crate) fn fold(reasons: &[(String, String)]) -> Phrase {
    let mut groups = groups(reasons);
    let named = |ats: &[&str]| ats.iter().filter(|at| !at.is_empty()).count();
    let resolvers = reasons.iter().filter(|(at, _)| !at.is_empty()).count();
    match &groups[..] {
        [] => Phrase::of("no resolver answered"),
        [(cause, ats)] => match &ats[..] {
            [""] => Phrase::of(without_os_error(cause)),
            [at] => Phrase::new("", vec![(format!("{at}: "), without_os_error(cause))]),
            _ => Phrase::new(
                format!("each of {resolvers} resolvers failed: "),
                vec![(String::new(), without_os_error(cause))],
            ),
        },
        _ => {
            // A stable sort: groups of one size keep the order of the asking,
            // and a failure of no resolver (out of time) goes last.
            groups.sort_by_key(|(_, ats)| std::cmp::Reverse(named(ats)));
            let causes = groups
                .iter()
                .map(|(cause, ats)| {
                    let lead = match named(ats) {
                        0 => String::new(),
                        n => format!("{n} with "),
                    };
                    (lead, without_os_error(cause))
                })
                .collect();
            let noun = if resolvers == 1 { "resolver" } else { "resolvers" };
            Phrase::new(format!("{resolvers} {noun} failed: "), causes)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;
    use std::time::Duration;

    use super::*;

    /// Six resolvers that fail alike give one short line with the count; a
    /// resolver that fails in other words is named alone
    /// (PodUtils/podssh#313).
    #[test]
    fn the_failures_that_read_alike_are_said_once() {
        let ips = ["1.1.1.1", "1.0.0.1", "8.8.8.8", "8.8.4.4", "9.9.9.9", "149.112.112.112"];
        let tls = |ip: &str| {
            format!(
                "{ip}: TLS: invalid peer certificate: certificate not valid for name \"{ip}\"; certificate is only \
                 valid for DnsName(\"default.domain\")"
            )
        };
        let all: Vec<(String, String)> = ips.iter().map(|ip| (ip.to_string(), tls(ip))).collect();
        let line = fold(&all).to_string();
        assert!(line.starts_with("each of 6 resolvers failed: TLS: invalid peer certificate"), "{line}");
        assert!(line.len() < 200, "{} characters: {line}", line.len());
        assert!(!line.contains("1.1.1.1"), "{line}");
        let mut mixed = all.clone();
        mixed[5] = ("149.112.112.112".to_string(), "149.112.112.112: timed out".to_string());
        mixed.push((String::new(), "out of time".to_string()));
        let line = each(&mixed);
        assert!(line.starts_with("5 resolvers (1.1.1.1, 1.0.0.1, 8.8.8.8, 8.8.4.4, 9.9.9.9): TLS"), "{line}");
        assert!(line.contains("; 149.112.112.112: timed out; out of time"), "{line}");
        let line = fold(&mixed).to_string();
        assert!(line.starts_with("6 resolvers failed: 5 with TLS: invalid peer certificate"), "{line}");
        assert!(line.ends_with("; 1 with timed out; out of time"), "each group once, with its count: {line}");
    }

    /// The failures of PodUtils/podnet#698: the system resolver, then DNS
    /// over HTTPS at eight resolvers with no IPv6 and no route, on a host
    /// that has no network out; and a host whose proxy re-signs TLS. The
    /// line of the dialer is one phrase for each source, in less than 200
    /// bytes (PodUtils/podnet#707, ask 7).
    #[tokio::test]
    async fn a_name_that_does_not_resolve_gives_one_short_line() {
        let v6 = ["2606:4700:4700::1111", "2606:4700:4700::1001", "2001:4860:4860::8888", "2001:4860:4860::8844"];
        let v4 = ["1.1.1.1", "1.0.0.1", "8.8.8.8", "8.8.4.4"];
        let mut lab: Vec<(String, String)> = v6
            .iter()
            .map(|ip| {
                let why =
                    format!("could not connect to [{ip}]:443: Address family not supported by protocol (os error 97)");
                (ip.to_string(), why)
            })
            .collect();
        lab.extend(v4.iter().map(|ip| {
            (ip.to_string(), format!("could not connect to {ip}:443: Network is unreachable (os error 101)"))
        }));
        let tls = |ip: &str| {
            format!(
                "{ip}: TLS: invalid peer certificate: certificate not valid for name \"{ip}\"; certificate is only \
                 valid for default.domain"
            )
        };
        let mut plain = lab[..4].to_vec();
        plain.extend(v4.iter().map(|ip| (ip.to_string(), tls(ip))));
        for reasons in [lab, plain] {
            let known = crate::order::Known { asks: true, ..Default::default() };
            let system = async { Err("failed to lookup address information: Try again".to_string()) };
            let doh = async { Err(fold(&reasons)) };
            let detail = crate::order::resolve_in(&known, 80, None, system, doh).await.unwrap_err();
            let line = pod_proxy::DialError::Resolve { host: "nowhere.invalid".into(), detail }.to_string();
            assert!(line.len() < 200, "{} bytes: {line}", line.len());
            assert!(line.contains("system resolver: Try again") && line.contains("DNS over HTTPS: "), "{line}");
            assert_eq!(line.matches("DNS over HTTPS: ").count(), 1, "{line}");
            assert!(!line.contains("resolvers ("), "one phrase for each source: {line}");
            assert!(!line.contains("2606:4700"), "{line}");
        }
    }

    fn deadline() -> Instant {
        Instant::now() + Duration::from_secs(10)
    }

    /// podnet's servers: Cloudflare (two), then Google (two), by address.
    fn servers() -> Vec<Doh> {
        let doh = |ip: &str, operator: &str| Doh::new(ip.parse().unwrap(), "doh.test").operator(operator).json("/q");
        vec![
            doh("1.1.1.1", "Cloudflare"),
            doh("1.0.0.1", "Cloudflare"),
            doh("8.8.8.8", "Google"),
            doh("8.8.4.4", "Google"),
        ]
    }

    fn by_address(servers: &[Doh]) -> Vec<(usize, String)> {
        servers.iter().enumerate().map(|(i, s)| (i, s.ip.to_string())).collect()
    }

    fn nx(at: &str) -> Missed {
        Missed::words(format!("{at}: {NXDOMAIN}"))
    }

    fn ips(list: &[&str]) -> Vec<IpAddr> {
        list.iter().map(|s| s.parse().unwrap()).collect()
    }

    // podnet's issue 119: a new name is NXDOMAIN at Cloudflare and has an
    // address at Google. Before, the first NXDOMAIN stopped the chain.
    #[tokio::test]
    async fn nxdomain_at_one_operator_asks_the_next_operator() {
        let servers = servers();
        let asked = Mutex::new(Vec::new());
        let rule = Nxdomain::NextOperator;
        let found = over(&servers, by_address(&servers), rule, "new.example", deadline(), |i, at| {
            asked.lock().unwrap().push(at.clone());
            let operator = servers[i].operator.clone();
            async move {
                match operator.as_str() {
                    "Cloudflare" => Err(nx(&at)),
                    _ => Ok((ips(&["192.0.2.9"]), Form::Json)),
                }
            }
        })
        .await;
        assert_eq!(found.unwrap(), ips(&["192.0.2.9"]));
        // The second server of Cloudflare is not asked; the first of Google
        // answered.
        let asked = asked.into_inner().unwrap();
        assert_eq!(asked, ["1.1.1.1", "8.8.8.8"], "{asked:?}");
        // podssh's rule ends the lookup at the first NXDOMAIN.
        let found = over(&servers, by_address(&servers), Nxdomain::End, "new.example", deadline(), |i, at| {
            let operator = servers[i].operator.clone();
            async move { if operator == "Cloudflare" { Err(nx(&at)) } else { Ok((ips(&["192.0.2.9"]), Form::Json)) } }
        })
        .await;
        assert_eq!(found.unwrap_err().to_string(), format!("1.1.1.1: {NXDOMAIN}"));
    }

    #[tokio::test]
    async fn nxdomain_at_each_operator_is_an_error_that_names_them() {
        let servers = servers();
        let rule = Nxdomain::NextOperator;
        let err =
            over(&servers, by_address(&servers), rule, "no.example", deadline(), |_, at| async move { Err(nx(&at)) })
                .await
                .unwrap_err();
        assert_eq!(err.to_string(), "Cloudflare and Google say that no.example does not exist");
    }

    #[tokio::test]
    async fn a_server_that_fails_is_skipped() {
        let servers = servers();
        let rule = Nxdomain::NextOperator;
        let found = over(&servers, by_address(&servers), rule, "x.example", deadline(), |_, at| async move {
            if at == "1.1.1.1" {
                return Err(Missed::words(format!("{at}: timed out")));
            }
            Ok((ips(&["2001:db8::1"]), Form::Json))
        })
        .await;
        assert_eq!(found.unwrap(), ips(&["2001:db8::1"]));
    }
}
