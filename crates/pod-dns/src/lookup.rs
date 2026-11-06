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
            reasons.push("out of time".to_string());
            break;
        }
        match ask(i, at).await {
            Ok((ips, _)) => return Ok(ips),
            Err(e) if e.why.ends_with(NXDOMAIN) && rule == Nxdomain::End => return Err(e.why),
            Err(e) if e.why.ends_with(NXDOMAIN) => {
                said.push(operator);
                reasons.push(e.why);
            }
            Err(e) => reasons.push(e.why),
        }
    }
    if !said.is_empty() {
        let verb = if said.len() > 1 { "say" } else { "says" };
        return Err(format!("{} {verb} that {host} does not exist", said.join(" and ")));
    }
    Err(if reasons.is_empty() { "no resolver answered".into() } else { reasons.join("; ") })
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;
    use std::time::Duration;

    use super::*;

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
