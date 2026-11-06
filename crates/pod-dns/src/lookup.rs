//! The addresses of a name from the resolvers in turn: the first resolver
//! that answers with addresses wins, and the first that says the name does
//! not exist ends the lookup.

use std::future::Future;
use std::net::IpAddr;

use pod_proxy::Missed;
use tokio::time::Instant;

use crate::NXDOMAIN;
use crate::doh::Form;

/// The lookup over the hosts of `order` (each the index of its resolver,
/// and its address or its name), with `ask` asking one of them.
pub(crate) async fn over<F, Fut>(order: Vec<(usize, String)>, deadline: Instant, ask: F) -> Result<Vec<IpAddr>, String>
where
    F: Fn(usize, String) -> Fut,
    Fut: Future<Output = Result<(Vec<IpAddr>, Form), Missed>>,
{
    let mut reasons = Vec::new();
    for (i, at) in order {
        if Instant::now() >= deadline {
            reasons.push("out of time".to_string());
            break;
        }
        match ask(i, at).await {
            Ok((ips, _)) => return Ok(ips),
            Err(e) if e.why.ends_with(NXDOMAIN) => return Err(e.why),
            Err(e) => reasons.push(e.why),
        }
    }
    Err(if reasons.is_empty() { "no resolver answered".into() } else { reasons.join("; ") })
}
