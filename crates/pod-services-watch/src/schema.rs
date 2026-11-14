//! The arguments of each watch kind, checked with no network (T-324): a
//! watch of the data with a missing, an unknown or a misnamed argument
//! fails `cargo test`, not the daily run against the live service. The
//! table "The schema" of `crates/pod-services/README.md` states the same.
//! The facts of a service kind with a table ([`kind_facts`], T-416) are
//! checked the same way.

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
        "stun-binding" | "ssh-banner" | "transit-relay" => args(&["endpoint"], &[], &[]),
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

/// The facts and the endpoints of one service kind (T-416): a service of
/// the kind needs each endpoint and each fact of `need`, can have each of
/// `optional`, and has no other fact. Each fact has its type.
#[derive(Debug, Clone, Copy)]
pub struct KindFacts {
    pub endpoints: &'static [&'static str],
    pub need: &'static [(&'static str, &'static str)],
    pub optional: &'static [(&'static str, &'static str)],
    /// The words that a fact of the type `text` can be.
    pub words: &'static [(&'static str, &'static [&'static str])],
    /// The facts that a word of a fact needs: (fact, word, the facts).
    pub when: &'static [(&'static str, &'static str, &'static [&'static str])],
}

/// The table of the kind `kind`, or none for a kind with no table: the
/// files of such a kind are held by the tests of `pod-services`.
pub fn kind_facts(kind: &str) -> Option<KindFacts> {
    Some(match kind {
        // The public file hosts (PodUtils/podssh#220): the upload form,
        // the answer, where the file is fetched, the delete, the limits.
        "file-host" => KindFacts {
            endpoints: &["upload"],
            need: &[
                ("file-field", "text"),
                ("fields", "texts"),
                ("life-seconds", "int"),
                ("answer", "text"),
                ("serves", "texts"),
                ("fetch-segment", "text"),
                ("fetch-method", "text"),
                ("delete", "text"),
                ("upload-bytes-most", "int"),
                ("one-person", "bool"),
                ("automated-uploads-refused", "bool"),
                // False: a road takes the host only when its user names it
                // (T-528).
                ("on-by-default", "bool"),
            ],
            optional: &[
                ("answer-pointer", "text"),
                // The file is a GET of the link in the page at the URL of the
                // answer that starts with this prefix (T-528).
                ("fetch-link-prefix", "text"),
                ("delete-token-header", "text"),
                ("delete-token-field", "text"),
                ("delete-fields", "texts"),
                ("uploads-per-hour", "int"),
                ("user-agent-names-program", "bool"),
                ("note", "text"),
            ],
            words: &[
                ("answer", &["text", "json"]),
                ("fetch-method", &["GET", "POST"]),
                ("delete", &["none", "token-form"]),
            ],
            when: &[
                ("answer", "json", &["answer-pointer"]),
                ("delete", "token-form", &["delete-token-header", "delete-token-field", "delete-fields"]),
            ],
        },
        // magic-wormhole's transit relays (PodUtils/podnet#535): the scheme
        // of `relay` tells TCP (`tcp`) from WebSocket (`wss`); no fact.
        "transit" => KindFacts { endpoints: &["relay"], need: &[], optional: &[], words: &[], when: &[] },
        _ => return None,
    })
}

/// The type of a value, as the key `type` of a fact names it.
fn type_of(value: &pod_services::Value) -> &'static str {
    use pod_services::Value;
    match value {
        Value::Text(_) => "text",
        Value::Int(_) => "int",
        Value::Bool(_) => "bool",
        Value::Date(_) => "date",
        Value::Texts(_) => "texts",
        Value::Ints(_) => "ints",
    }
}

/// Each fault of `service` against the table of its kind: a missing
/// endpoint, a missing fact, a fact of another type or of a word that the
/// table does not name, a fact that the table does not name, and a fact
/// that a word needs. None for a kind with no table.
pub fn service_faults(service: &Service) -> Vec<String> {
    let Some(table) = kind_facts(service.kind()) else { return Vec::new() };
    let mut out = Vec::new();
    for name in table.endpoints.iter().filter(|n| service.endpoint(n).is_none()) {
        out.push(format!("no endpoint {name:?} of the kind {:?}", service.kind()));
    }
    for (name, kind) in table.need {
        if service.fact(name).is_none() {
            out.push(format!("no fact {name:?} ({kind}) of the kind {:?}", service.kind()));
        }
    }
    for fact in service.facts() {
        let named = table.need.iter().chain(table.optional).find(|(n, _)| *n == fact.name());
        match named {
            None => out.push(format!("the fact {:?} is not one of the kind {:?}", fact.name(), service.kind())),
            Some((_, kind)) if *kind != type_of(fact.value()) => {
                out.push(format!("the fact {:?} is of the type {}, not {kind}", fact.name(), type_of(fact.value())));
            }
            Some(_) => {}
        }
    }
    for (name, words) in table.words {
        if let Some(word) = service.text(name).filter(|w| !words.contains(w)) {
            out.push(format!("the fact {name:?} is {word:?}, not one of {}", words.join(", ")));
        }
    }
    for (name, word, needed) in table.when {
        if service.text(name) == Some(*word) {
            for missing in needed.iter().filter(|n| service.fact(n).is_none()) {
                out.push(format!("the fact {name:?} {word:?} needs the fact {missing:?}"));
            }
        }
    }
    out
}
