//! The DNS watches, over the resolvers of DNS over HTTPS of the run:
//! `dns-srv` (an SRV record), `dns-addresses` (the address records of the
//! hosts of a role) and `doh-answer` (a resolver of the service answers in
//! one form).

use std::collections::BTreeSet;
use std::net::IpAddr;

use pod_dns::Doh;
use pod_services::{Service, Watch};

use crate::args::{Args, named, value_texts};
use crate::{Checked, Finding, Net, Outcome};

fn key(host: &str) -> String {
    host.trim_end_matches('.').to_ascii_lowercase()
}

/// `dns-srv`: the SRV record of the name of `fact` names the hosts of the
/// endpoints of `role`, each with the port of its endpoint.
pub async fn srv(service: &Service, watch: &Watch, net: &Net) -> Checked {
    let args = Args::new(service, watch);
    let fact = args.fact("fact")?;
    let name = fact.value().as_text().ok_or_else(|| Finding::failed("the fact of the SRV name is no text"))?;
    let records = net.way().srv(name, net.deadline()).await.map_err(|e| Finding::failed(format!("SRV {name}: {e}")))?;
    let live: BTreeSet<(String, u16)> = records.iter().map(|r| (key(&r.target), r.port)).collect();
    let file: BTreeSet<(String, u16)> = args.endpoints()?.iter().map(|e| (key(e.host()), e.port())).collect();
    let mut out = Outcome::default();
    let shown = |s: &BTreeSet<(String, u16)>| s.iter().map(|(h, p)| format!("{h}:{p}")).collect::<Vec<_>>().join(", ");
    if live == file {
        out.saw(format!("SRV {name} names {}", shown(&live)));
    } else {
        out.found(Finding::moved(format!("the targets of {}", named(fact)), shown(&file), shown(&live)));
    }
    Ok(out)
}

/// `dns-addresses`: the address records of the hosts of the endpoints of
/// `role` hold each address of each fact of `facts`.
pub async fn addresses(service: &Service, watch: &Watch, net: &Net) -> Checked {
    let args = Args::new(service, watch);
    let mut live = BTreeSet::new();
    let mut hosts = Vec::new();
    for endpoint in args.endpoints()? {
        let host = key(endpoint.host());
        let found =
            net.way().lookup(&host, net.deadline()).await.map_err(|e| Finding::failed(format!("{host}: {e}")))?;
        live.extend(found);
        hosts.push(host);
    }
    let mut out = Outcome::default();
    for name in args.texts("facts")? {
        let fact = service.fact(name).ok_or_else(|| Finding::failed(format!("the service has no fact {name:?}")))?;
        for address in value_texts(fact.value()) {
            match address.parse::<IpAddr>() {
                Ok(ip) if live.contains(&ip) => out.saw(format!("{ip} is an address of {}", hosts.join(", "))),
                Ok(ip) => {
                    out.found(Finding::moved(named(fact), ip.to_string(), format!("no record of {}", hosts.join(", "))))
                }
                Err(_) => out.found(Finding::failed(format!("{address:?} of {} is no address", named(fact)))),
            }
        }
    }
    Ok(out)
}

/// `doh-answer`: each resolver of the service (each address of each
/// endpoint) answers a question of `type` for `name` in `form` (`json` or
/// `wire`) with an address.
pub async fn doh_answer(service: &Service, watch: &Watch, net: &Net) -> Checked {
    let args = Args::new(service, watch);
    let (form, name, kind) = (args.text("form")?, args.text("name")?, args.text("type")?);
    let want_v4 = match kind {
        "A" => true,
        "AAAA" => false,
        other => return Err(Finding::failed(format!("doh-answer asks A or AAAA, not {other}"))),
    };
    let roles: Vec<&str> = service.endpoints().iter().filter_map(|e| e.role()).collect();
    let resolvers = Doh::of_service(service, &roles);
    if resolvers.is_empty() {
        return Err(Finding::failed("the service has no resolver with an address"));
    }
    let mut out = Outcome::default();
    for resolver in resolvers {
        let resolver = match form {
            "json" if resolver.json.is_some() => Doh { wire: None, ..resolver },
            "wire" if resolver.wire.is_some() => Doh { json: None, ..resolver },
            _ => {
                out.found(Finding::failed(format!("{} serves no {form} form", resolver.ip)));
                continue;
            }
        };
        let at = format!("{} ({})", resolver.ip, resolver.name);
        let resolvers = [resolver];
        match net.way().lookup_with(&resolvers, name, net.deadline()).await {
            Ok(ips) if ips.iter().any(|ip| ip.is_ipv4() == want_v4) => {
                out.saw(format!("{at} answered {kind} {name} in the {form} form"));
            }
            Ok(_) => out.found(Finding::failed(format!("{at} gave no {kind} record of {name}"))),
            Err(why) => out.found(Finding::failed(format!("{at}: {why}"))),
        }
    }
    Ok(out)
}
