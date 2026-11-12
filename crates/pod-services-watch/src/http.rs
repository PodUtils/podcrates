//! The watches of an HTTP answer: `http-json` (a value of a document, or
//! an item of a list for each endpoint),
//! `http-status` (the status of a GET), `http-text` (a text in an answer)
//! and `github-ssh-keys` (GitHub's published fingerprints).

use std::collections::BTreeSet;

use pod_services::{Service, Value, Watch};
use serde_json::Value as Json;

use crate::args::{Args, https_url, named};
use crate::net::shown;
use crate::{Checked, Finding, Net, Outcome};

/// `http-json`: the document at `url` has a value at `pointer`; with
/// `fact`, it equals the fact; with `facts` and `members`, each member of
/// it equals its fact; with `key`, it is a list with an item for each
/// endpoint (`items`); with `member`, the members of that name under it
/// hold the host of each endpoint (of `role`, or each endpoint).
pub async fn json(service: &Service, watch: &Watch, net: &Net) -> Checked {
    let args = Args::new(service, watch);
    let (url, pointer) = (args.text("url")?, args.text("pointer")?);
    let document = net.json(url).await?;
    let value = document
        .pointer(pointer)
        .ok_or_else(|| Finding::failed(format!("{} has no value at {pointer}", shown(url))))?;
    let mut out = Outcome::default();
    if let Some(fact) = args.opt_fact("fact")? {
        compare(&mut out, &named(fact), fact.value(), Some(value));
    } else if let (Ok(facts), Ok(members)) = (args.texts("facts"), args.texts("members")) {
        if facts.len() != members.len() {
            return Err(Finding::failed("the watch has a different count of facts and members"));
        }
        for (fact, member) in facts.iter().zip(&members) {
            let fact =
                service.fact(fact).ok_or_else(|| Finding::failed(format!("the service has no fact {fact:?}")))?;
            compare(&mut out, &named(fact), fact.value(), value.get(member));
        }
    } else if args.opt_text("key").is_some() {
        items(&mut out, &args, value, url, pointer)?;
    } else if let Some(member) = args.opt_text("member") {
        let mut live = BTreeSet::new();
        members(value, member, &mut live);
        let file: BTreeSet<String> = args.endpoints()?.iter().map(|e| host_key(e.host())).collect();
        for host in file.difference(&live) {
            out.found(Finding::moved(
                format!("the host {host}"),
                "an endpoint",
                format!("no {member} {host} at {pointer}"),
            ));
        }
        // The document may hold more hosts than the file (a pool holds each
        // relay; the seed is a part of it): they are counted, not found.
        let more = live.difference(&file).count();
        out.saw(format!("{} hosts at {} of {}, {more} of them not in the file", live.len(), pointer, shown(url)));
    }
    if out.seen.is_empty() {
        out.saw(format!("{} has a value at {pointer}", shown(url)));
    }
    Ok(out)
}

/// The form `key` of `http-json`: the value is a list, and each endpoint
/// (of `role`, or each endpoint) is the item whose member `key` is the text
/// of the fact `fact-prefix` + NAME. The item holds the endpoint's host in
/// a member of `member`, its port in the member `port-member` (a number or
/// a list), and `true` in each member of `true-members`.
fn items(out: &mut Outcome, args: &Args, value: &Json, url: &str, pointer: &str) -> Result<(), Finding> {
    let (key, prefix, hosts) = (args.text("key")?, args.text("fact-prefix")?, args.texts("member")?);
    let (port, flags) = (args.opt_text("port-member"), args.opt_texts("true-members")?);
    let Json::Array(list) = value else {
        return Err(Finding::failed(format!("{} has no list at {pointer}", shown(url))));
    };
    let endpoints = args.endpoints()?;
    let mut keys = BTreeSet::new();
    for endpoint in &endpoints {
        let name = format!("{prefix}{}", endpoint.name());
        let fact =
            args.service.fact(&name).ok_or_else(|| Finding::failed(format!("the service has no fact {name:?}")))?;
        let want = fact.value().as_text().ok_or_else(|| Finding::failed(format!("the fact {name:?} is no text")))?;
        keys.insert(want);
        let Some(item) = list.iter().find(|i| member_str(i, key) == Some(want)) else {
            out.found(Finding::moved(named(fact), want, format!("no item with the {key} {want} at {pointer}")));
            continue;
        };
        // A moved host, port or flag names the endpoint, so that its line
        // shows the endpoint's source.
        let at = format!("endpoint {}", endpoint.name());
        if !hosts.iter().filter_map(|m| member_str(item, m)).any(|h| host_key(h) == host_key(endpoint.host())) {
            let now: Vec<String> = hosts.iter().map(|m| format!("{m} {}", member_text(item, m))).collect();
            out.found(Finding::moved(&at, endpoint.host(), now.join(", ")));
        }
        if let Some(member) = port
            && !holds_port(item.get(member), endpoint.port())
        {
            let now = format!("{member} {}", member_text(item, member));
            out.found(Finding::moved(&at, format!("port {}", endpoint.port()), now));
        }
        for flag in &flags {
            if item.get(*flag) != Some(&Json::Bool(true)) {
                out.found(Finding::moved(&at, format!("{flag} true"), format!("{flag} {}", member_text(item, flag))));
            }
        }
    }
    // A list can hold more items that fit than the file needs (each public
    // node): they are counted, not found.
    let ports: BTreeSet<u16> = endpoints.iter().map(|e| e.port()).collect();
    let fits = |item: &&Json| {
        port.is_none_or(|m| ports.iter().any(|p| holds_port(item.get(m), *p)))
            && flags.iter().all(|f| item.get(*f) == Some(&Json::Bool(true)))
    };
    let fit: Vec<&Json> = list.iter().filter(fits).collect();
    let more = fit.iter().filter(|i| !member_str(i, key).is_some_and(|k| keys.contains(k))).count();
    let mut terms: Vec<String> = Vec::new();
    if let Some(member) = port {
        let ports: Vec<String> = ports.iter().map(u16::to_string).collect();
        terms.push(format!("{member} {}", ports.join(" or ")));
    }
    terms.extend(flags.iter().map(|f| format!("{f} true")));
    let with = if terms.is_empty() { String::new() } else { format!(" with {}", terms.join(" and ")) };
    out.saw(format!(
        "{} endpoints at {pointer} of {}; {} of {} items{with}, {more} of them not in the file",
        endpoints.len(),
        shown(url),
        fit.len(),
        list.len()
    ));
    Ok(())
}

/// True when `value` is the port, or a list that holds it.
fn holds_port(value: Option<&Json>, port: u16) -> bool {
    match value {
        Some(Json::Number(n)) => n.as_u64() == Some(u64::from(port)),
        Some(Json::Array(items)) => items.iter().any(|i| i.as_u64() == Some(u64::from(port))),
        _ => false,
    }
}

/// The text of the member `name` of `item`, if it is a text.
fn member_str<'j>(item: &'j Json, name: &str) -> Option<&'j str> {
    item.get(name).and_then(Json::as_str)
}

/// The JSON text of the member `name` of `item`, or `none`.
fn member_text(item: &Json, name: &str) -> String {
    item.get(name).map_or_else(|| "none".into(), Json::to_string)
}

/// A finding when `live` is not the fact's value.
fn compare(out: &mut Outcome, name: &str, fact: &Value, live: Option<&Json>) {
    let Some(live) = live else {
        out.found(Finding::moved(name, fact.to_string(), "no value"));
        return;
    };
    let same = match (fact, live) {
        (Value::Text(t), Json::String(s)) => t.as_ref() == s,
        (Value::Int(i), Json::Number(n)) => n.as_i64() == Some(*i),
        (Value::Bool(b), Json::Bool(l)) => b == l,
        (Value::Texts(list), Json::Array(items)) => {
            list.len() == items.len() && list.iter().zip(items).all(|(t, i)| i.as_str() == Some(t.as_ref()))
        }
        (Value::Ints(list), Json::Array(items)) => {
            list.len() == items.len() && list.iter().zip(items).all(|(t, i)| i.as_i64() == Some(*t))
        }
        _ => false,
    };
    if same {
        out.saw(format!("{name} is {fact}"));
    } else {
        out.found(Finding::moved(name, fact.to_string(), live.to_string()));
    }
}

/// Each text of a member `name` anywhere under `value`.
fn members(value: &Json, name: &str, out: &mut BTreeSet<String>) {
    match value {
        Json::Object(map) => {
            for (key, item) in map {
                match item {
                    Json::String(s) if key == name => {
                        out.insert(host_key(s));
                    }
                    _ => members(item, name, out),
                }
            }
        }
        Json::Array(items) => items.iter().for_each(|i| members(i, name, out)),
        _ => {}
    }
}

/// A host as a key to compare: a name in lower case with no last `.`, or
/// an IP address in its one text form (`2001:DB8:0::1` is `2001:db8::1`).
fn host_key(host: &str) -> String {
    let host = host.trim_end_matches('.');
    match host.parse::<std::net::IpAddr>() {
        Ok(address) => address.to_string(),
        Err(_) => host.to_ascii_lowercase(),
    }
}

/// `http-status`: a GET of each endpoint (of `endpoint` or `role`), at
/// `path` when given, answers `status`. The body is not read.
pub async fn status(service: &Service, watch: &Watch, net: &Net) -> Checked {
    let args = Args::new(service, watch);
    let want = args.int("status")?;
    let mut out = Outcome::default();
    for endpoint in args.endpoints()? {
        let url = https_url(endpoint, args.opt_text("path"));
        match net.streamed(net.request("GET", &url)).await {
            Ok(answer) if i64::from(answer.status) == want => out.saw(format!("{} answered {want}", shown(&url))),
            Ok(answer) => out.found(Finding::failed(format!("{} answered {}, not {want}", shown(&url), answer.status))),
            Err(finding) => out.found(finding),
        }
    }
    Ok(out)
}

/// `http-text`: the answer to a GET of the endpoint holds `contains`.
pub async fn text(service: &Service, watch: &Watch, net: &Net) -> Checked {
    let args = Args::new(service, watch);
    let (endpoint, contains) = (args.endpoint()?, args.text("contains")?);
    let url = https_url(endpoint, None);
    let answer = net.get_ok(&url, "*/*").await?;
    if !answer.text().contains(contains) {
        let shown = answer.body_text(120);
        return Err(Finding::failed(format!("{} does not hold {contains:?}: {shown}", self::shown(&url))));
    }
    Ok(Outcome { seen: vec![format!("{} holds {contains:?}", shown(&url))], findings: Vec::new(), skipped: None })
}

/// `github-ssh-keys`: the fingerprints at `pointer` of the document at
/// `url` (the texts of an object, with no `SHA256:`) are the fact's.
pub async fn github_ssh_keys(service: &Service, watch: &Watch, net: &Net) -> Checked {
    let args = Args::new(service, watch);
    let (url, pointer, fact) = (args.text("url")?, args.text("pointer")?, args.fact("fact")?);
    let document = net.json(url).await?;
    let Some(Json::Object(keys)) = document.pointer(pointer) else {
        return Err(Finding::failed(format!("{} has no object at {pointer}", shown(url))));
    };
    let live: BTreeSet<String> = keys
        .values()
        .filter_map(Json::as_str)
        .map(|k| if k.starts_with("SHA256:") { k.to_string() } else { format!("SHA256:{k}") })
        .collect();
    let file: BTreeSet<String> = crate::args::value_texts(fact.value()).into_iter().collect();
    let mut out = Outcome::default();
    if live == file {
        out.saw(format!("{} fingerprints at {pointer}", live.len()));
    } else {
        let join = |s: &BTreeSet<String>| s.iter().cloned().collect::<Vec<_>>().join(", ");
        out.found(Finding::moved(named(fact), join(&file), join(&live)));
    }
    Ok(out)
}
