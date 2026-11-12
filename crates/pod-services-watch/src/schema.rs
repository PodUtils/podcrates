//! The arguments of each watch kind, checked with no network (T-324): a
//! watch of the data with a missing, an unknown or a misnamed argument
//! fails `cargo test`, not the daily run against the live service. The
//! table "The schema" of `crates/pod-services/README.md` states the same.

use pod_services::{Service, Watch};

/// The arguments of one kind.
#[derive(Debug, Clone, Copy)]
pub struct Arguments {
    /// Each of these is needed.
    pub need: &'static [&'static str],
    /// One or more of each group is needed.
    pub any: &'static [&'static [&'static str]],
    /// These can be there.
    pub optional: &'static [&'static str],
}

const fn args(
    need: &'static [&'static str],
    any: &'static [&'static [&'static str]],
    optional: &'static [&'static str],
) -> Arguments {
    Arguments { need, any, optional }
}

const ENDPOINT_OR_ROLE: &[&[&str]] = &[&["endpoint", "role"]];

/// The arguments of the kind `kind`, or none for a kind that this crate
/// does not know.
pub fn arguments(kind: &str) -> Option<Arguments> {
    Some(match kind {
        "http-json" => args(
            &["url", "pointer"],
            &[],
            &[
                "fact",
                "facts",
                "members",
                "member",
                "role",
                "key",
                "fact-prefix",
                "port-member",
                "true-members",
                "url-member",
            ],
        ),
        "http-status" => args(&["status"], ENDPOINT_OR_ROLE, &["path"]),
        "http-text" => args(&["contains"], &[&["url", "endpoint"]], &[]),
        "github-ssh-keys" => args(&["url", "pointer", "fact"], &[], &[]),
        "http-sha256" => args(&["endpoint"], &[&["fact", "files"]], &["facts", "start", "sector", "bytes", "length"]),
        "tls-name" | "tls-names" => args(&[], ENDPOINT_OR_ROLE, &[]),
        "dns-srv" => args(&["fact", "role"], &[], &[]),
        "dns-addresses" => args(&["role", "facts"], &[], &[]),
        "doh-answer" => args(&["form", "name", "type"], &[], &[]),
        "stun-binding" | "ssh-banner" => args(&["endpoint"], &[], &[]),
        "websocket-upgrade" => args(&["endpoint"], &[], &["protocol"]),
        "forward-banner" => args(&["template", "target"], &[], &[]),
        "forward-url" => args(&["endpoint"], &[], &["port", "seconds", "prove"]),
        "proxy-banner" => args(&["fact", "target"], &[], &[]),
        "ssh-host-key" => args(&["endpoint", "fact"], &[], &[]),
        "github-release" => args(&["repo", "fact", "max-age-days"], &[], &[]),
        "github-files" => args(&["repo", "tag", "fact"], &[], &[]),
        "github-file" => args(&["repo", "path"], &[&["endpoint", "facts"]], &[]),
        "tls-roots" => args(&["repo", "path", "tag", "fact"], &[], &[]),
        "crate-file" => args(&["crate", "path"], ENDPOINT_OR_ROLE, &[]),
        "rdap" => args(&["domain"], &[], &["ahead-days"]),
        "oci-ping" => args(&["endpoint"], &[], &["fact"]),
        "oci-digest" => args(&["endpoint", "facts"], &[], &[]),
        _ => return None,
    })
}

/// The arguments whose values name a fact of the service.
const FACT_NAMES: &[&str] = &["fact", "facts", "tag", "template", "length", "start", "sector", "bytes"];

/// Each fault of the arguments of `watch` of `service`: a kind with no
/// table, a missing or an unknown argument, and a name of an endpoint, a
/// role, a fact or a service (`is_service`) that is not there.
pub fn faults(service: &Service, watch: &Watch, is_service: impl Fn(&str) -> bool) -> Vec<String> {
    let Some(spec) = arguments(watch.kind()) else {
        return vec![format!("no table of the arguments of the kind {:?}", watch.kind())];
    };
    let has = |name: &str| watch.arg(name).is_some();
    let mut out = Vec::new();
    for name in spec.need.iter().filter(|n| !has(n)) {
        out.push(format!("no argument {name:?}"));
    }
    for group in spec.any.iter().filter(|g| !g.iter().any(|n| has(n))) {
        out.push(format!("no argument of {}", group.join(" or ")));
    }
    let known = |name: &str| {
        spec.need.contains(&name) || spec.optional.contains(&name) || spec.any.iter().any(|g| g.contains(&name))
    };
    for arg in watch.args().iter().filter(|a| !known(a.name())) {
        out.push(format!("an unknown argument {:?}", arg.name()));
    }
    let texts = |name: &str| watch.arg(name).map(crate::args::value_texts).unwrap_or_default();
    for name in texts("endpoint").iter().filter(|n| service.endpoint(n).is_none()) {
        out.push(format!("no endpoint {name:?}"));
    }
    for role in texts("role").iter().filter(|r| service.endpoints_of(r).next().is_none()) {
        out.push(format!("no endpoint of the role {role:?}"));
    }
    for arg in FACT_NAMES {
        for name in texts(arg).iter().filter(|n| service.fact(n).is_none()) {
            out.push(format!("the argument {arg:?} names no fact {name:?}"));
        }
    }
    for id in texts("target").iter().filter(|id| !is_service(id)) {
        out.push(format!("the argument \"target\" names no service {id:?}"));
    }
    out
}
