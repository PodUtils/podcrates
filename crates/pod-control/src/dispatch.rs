//! The dispatch: one table for the local methods and one for the delegated
//! methods, each keyed by the wire name. A request finds its method in the
//! catalogue first, at the tier of the caller; the tests hold each table to
//! the catalogue in both directions.

use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use crate::catalogue::{Catalogue, Entry, Route, Tier};
use crate::host::{Context, Host, Outcome};
use crate::server::Settings;
use crate::wire::{Code, Fault, PROTOCOL};

/// The state of one connection that the local methods read and change.
pub(crate) struct Session<'a> {
    pub settings: &'a Settings,
    pub catalogue: &'a Catalogue,
    pub tier: Tier,
    /// `auth` was tried on this connection: one try for each connection.
    pub auth_tried: bool,
    /// Close the connection after this answer.
    pub close: bool,
}

pub(crate) type LocalFn = fn(&mut Session<'_>, &Value) -> Outcome;
pub(crate) type DelegatedFn = fn(&mut dyn Host, &Value, &mut Context) -> Outcome;

/// The methods that the server answers.
pub(crate) const LOCAL: &[(&str, LocalFn)] = &[("version", version), ("methods", methods), ("auth", auth)];

/// The methods that the host answers.
pub(crate) const DELEGATED: &[(&str, DelegatedFn)] = &[
    ("status", |h, p, cx| none(p).and_then(|()| h.status(cx))),
    ("metrics", |h, p, cx| {
        none(p)?;
        h.metrics(cx).map(|m| serde_json::to_value(m).unwrap_or(Value::Null))
    }),
    ("reload", |h, p, cx| none(p).and_then(|()| h.reload(cx))),
    ("drain", |h, p, cx| typed(p).and_then(|d| h.drain(d, cx))),
    ("stop", |h, p, cx| none(p).and_then(|()| h.stop(cx))),
];

/// A method found for a request.
pub(crate) enum Target {
    Local(LocalFn),
    Delegated(DelegatedFn),
    Extra(String),
}

/// The target of `name` for a caller of `tier`. A name that is not there and
/// a method above the tier give the same fault.
pub(crate) fn find(catalogue: &Catalogue, name: &str, tier: Tier) -> Result<Target, Fault> {
    let entry = catalogue.find_for(name, tier).ok_or_else(Fault::unknown_method)?;
    match entry {
        Entry::Extra(e) => Ok(Target::Extra(e.name.clone())),
        Entry::Core(spec) => match spec.route {
            Route::Local => LOCAL.iter().find(|(n, _)| *n == name).map(|(_, f)| Target::Local(*f)),
            Route::Delegated => DELEGATED.iter().find(|(n, _)| *n == name).map(|(_, f)| Target::Delegated(*f)),
        }
        .ok_or_else(|| Fault::new(Code::Internal, "a method of the catalogue has no handler")),
    }
}

/// Answer `target` with the host: the work of the host thread.
pub(crate) fn delegate(target: &Target, host: &mut dyn Host, params: &Value, cx: &mut Context) -> Outcome {
    match target {
        Target::Delegated(f) => f(host, params, cx),
        Target::Extra(name) => host.extra(name, params, cx),
        Target::Local(_) => Err(Fault::new(Code::Internal, "a local method went to the host")),
    }
}

fn bad_params() -> Fault {
    Fault::new(Code::BadParams, "the params do not fit the method")
}

/// The params of a method that takes none: an empty object.
pub(crate) fn none(params: &Value) -> Result<(), Fault> {
    match params.as_object() {
        Some(m) if m.is_empty() => Ok(()),
        _ => Err(bad_params()),
    }
}

/// The params as `T`, read in place: a clone would be one more copy to
/// wipe. The words of serde can quote a value: they do not go back.
pub(crate) fn typed<T: DeserializeOwned>(params: &Value) -> Result<T, Fault> {
    T::deserialize(params).map_err(|_| bad_params())
}

fn version(s: &mut Session<'_>, params: &Value) -> Outcome {
    none(params)?;
    Ok(json!({ "program": s.settings.program, "version": s.settings.version, "protocol": PROTOCOL }))
}

fn methods(s: &mut Session<'_>, params: &Value) -> Outcome {
    none(params)?;
    let rows: Vec<Value> = s
        .catalogue
        .entries()
        .filter(|e| e.tier() <= s.tier)
        .map(
            |e| json!({ "name": e.name(), "tier": e.tier().name(), "route": e.route().name(), "summary": e.summary() }),
        )
        .collect();
    Ok(json!({ "tier": s.tier.name(), "methods": rows }))
}

/// The token of `auth` is read in place in the params, with no copy: the
/// params are wiped after the answer.
fn auth(s: &mut Session<'_>, params: &Value) -> Outcome {
    let token = match params.as_object() {
        Some(map) if map.len() == 1 => map.get("token").and_then(Value::as_str),
        _ => None,
    }
    .ok_or_else(bad_params)?;
    let Some(master) = &s.settings.master else {
        return Err(Fault::new(Code::Refused, "this program has no master token"));
    };
    if s.auth_tried {
        s.close = true;
        return Err(Fault::new(Code::Refused, "one try of the token for each connection"));
    }
    s.auth_tried = true;
    if !master.matches(token) {
        s.close = true;
        return Err(Fault::new(Code::Refused, "the token does not match"));
    }
    s.tier = Tier::Master;
    Ok(json!({ "tier": Tier::Master.name() }))
}
