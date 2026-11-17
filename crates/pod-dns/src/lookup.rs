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
use crate::settings::Nxdomain;

/// The lookup over the hosts of `order` (each the index of its resolver in
/// `resolvers`, and its address or its name), with `ask` asking one of them.
pub(crate) async fn over<F, Fut>(
    resolvers: &[Doh],
    order: Vec<(usize, String)>,
    rule: Nxdomain,
    host: &str,
    deadline: Instant,
    ask: F,
) -> Result<Vec<IpAddr>, String>
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
            Err(e) if e.why.ends_with(NXDOMAIN) && rule == Nxdomain::End => return Err(e.why),
            Err(e) if e.why.ends_with(NXDOMAIN) => {
                said.push(operator);
                reasons.push((at, e.why));
            }
            Err(e) => reasons.push((at, e.why)),
        }
    }
    if !said.is_empty() {
        let verb = if said.len() > 1 { "say" } else { "says" };
        return Err(format!("{} {verb} that {host} does not exist", said.join(" and ")));
    }
    Err(if reasons.is_empty() { "no resolver answered".into() } else { fold(&reasons) })
}

/// One line of the failures of the resolvers, each `(address, why)`: a
/// failure that each resolver had in the same words is said once with the
/// count, and a resolver is named alone only where its failure differs
/// (PodUtils/podssh#313). The address of a resolver in its own words is
/// taken out first, so that a TLS failure that names it reads alike.
pub(crate) fn fold(reasons: &[(String, String)]) -> String {
    let mut groups: Vec<(String, Vec<&str>)> = Vec::new();
    for (at, why) in reasons {
        let cause = if at.is_empty() {
            why.clone()
        } else {
            let rest = why.strip_prefix(at.as_str()).map_or(why.as_str(), |r| r.trim_start_matches(':').trim_start());
            rest.replace(at.as_str(), "the address of the resolver")
        };
        match groups.iter_mut().find(|g| g.0 == cause) {
            Some(g) => g.1.push(at),
            None => groups.push((cause, vec![at])),
        }
    }
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
        let line = fold(&all);
        assert!(line.starts_with("each of 6 resolvers failed: TLS: invalid peer certificate"), "{line}");
        assert!(line.len() < 200, "{} characters: {line}", line.len());
        assert!(!line.contains("1.1.1.1"), "{line}");
        let mut mixed = all.clone();
        mixed[5] = ("149.112.112.112".to_string(), "149.112.112.112: timed out".to_string());
        mixed.push((String::new(), "out of time".to_string()));
        let line = fold(&mixed);
        assert!(line.starts_with("5 resolvers (1.1.1.1, 1.0.0.1, 8.8.8.8, 8.8.4.4, 9.9.9.9): TLS"), "{line}");
        assert!(line.contains("; 149.112.112.112: timed out; out of time"), "{line}");
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
        assert_eq!(found.unwrap_err(), format!("1.1.1.1: {NXDOMAIN}"));
    }

    #[tokio::test]
    async fn nxdomain_at_each_operator_is_an_error_that_names_them() {
        let servers = servers();
        let rule = Nxdomain::NextOperator;
        let err =
            over(&servers, by_address(&servers), rule, "no.example", deadline(), |_, at| async move { Err(nx(&at)) })
                .await
                .unwrap_err();
        assert_eq!(err, "Cloudflare and Google say that no.example does not exist");
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
