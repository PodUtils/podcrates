//! The watches of an HTTP answer: `http-json` (a value of a document),
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
/// it equals its fact; with `member`, the members of that name under it
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

fn host_key(host: &str) -> String {
    host.trim_end_matches('.').to_ascii_lowercase()
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
