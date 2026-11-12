//! The arguments of a `[[watch]]` and the values that they name: an
//! argument that is missing or of another type is a failure of the watch,
//! with its name.

use pod_services::{Endpoint, Fact, Service, Value};

use crate::Finding;

/// The arguments of one watch of one service.
#[derive(Clone, Copy)]
pub struct Args<'a> {
    pub service: &'a Service,
    pub watch: &'a pod_services::Watch,
}

impl<'a> Args<'a> {
    pub fn new(service: &'a Service, watch: &'a pod_services::Watch) -> Args<'a> {
        Args { service, watch }
    }

    /// The text of the argument `name`, if any.
    pub fn opt_text(&self, name: &str) -> Option<&'a str> {
        self.watch.arg(name).and_then(Value::as_text)
    }

    /// The text of the argument `name`.
    pub fn text(&self, name: &str) -> Result<&'a str, Finding> {
        self.opt_text(name).ok_or_else(|| missing(name, "a text"))
    }

    /// The integer of the argument `name`.
    pub fn int(&self, name: &str) -> Result<i64, Finding> {
        self.watch.arg(name).and_then(Value::as_int).ok_or_else(|| missing(name, "an integer"))
    }

    /// The texts of the argument `name`: a list, or one text.
    pub fn texts(&self, name: &str) -> Result<Vec<&'a str>, Finding> {
        match self.watch.arg(name) {
            Some(Value::Texts(list)) => Ok(list.iter().map(|t| t.as_ref()).collect()),
            Some(Value::Text(text)) => Ok(vec![text.as_ref()]),
            _ => Err(missing(name, "a text or a list of texts")),
        }
    }

    /// The texts of the argument `name`, if the watch has the argument;
    /// else none.
    pub fn opt_texts(&self, name: &str) -> Result<Vec<&'a str>, Finding> {
        match self.watch.arg(name) {
            Some(_) => self.texts(name),
            None => Ok(Vec::new()),
        }
    }

    /// The fact that the argument `name` names.
    pub fn fact(&self, name: &str) -> Result<&'a Fact, Finding> {
        let fact = self.text(name)?;
        self.service.fact(fact).ok_or_else(|| Finding::failed(format!("the service has no fact {fact:?}")))
    }

    /// The fact that the argument `name` names, if the watch has the
    /// argument.
    pub fn opt_fact(&self, name: &str) -> Result<Option<&'a Fact>, Finding> {
        match self.opt_text(name) {
            Some(_) => self.fact(name).map(Some),
            None => Ok(None),
        }
    }

    /// The endpoint that the argument `endpoint` names; else each endpoint
    /// of the role that the argument `role` names; else each endpoint.
    pub fn endpoints(&self) -> Result<Vec<&'a Endpoint>, Finding> {
        if let Some(name) = self.opt_text("endpoint") {
            let endpoint = self.service.endpoint(name);
            return endpoint
                .map(|e| vec![e])
                .ok_or_else(|| Finding::failed(format!("the service has no endpoint {name:?}")));
        }
        let all: Vec<&Endpoint> = match self.opt_text("role") {
            Some(role) => self.service.endpoints_of(role).collect(),
            None => self.service.endpoints().iter().collect(),
        };
        if all.is_empty() {
            return Err(Finding::failed("the watch names no endpoint of the service"));
        }
        Ok(all)
    }

    /// The one endpoint of the argument `endpoint`.
    pub fn endpoint(&self) -> Result<&'a Endpoint, Finding> {
        let name = self.text("endpoint")?;
        self.service.endpoint(name).ok_or_else(|| Finding::failed(format!("the service has no endpoint {name:?}")))
    }

    /// The service that the argument `name` names, from the services of the
    /// run.
    pub fn target<'n>(&self, name: &str, services: &'n pod_services::Catalog) -> Result<&'n Service, Finding> {
        let id = self.text(name)?;
        services.get(id).ok_or_else(|| Finding::failed(format!("no service has the id {id:?}")))
    }
}

fn missing(name: &str, what: &str) -> Finding {
    Finding::failed(format!("the watch has no argument {name:?} of {what}"))
}

/// The texts of a value as a source file writes them: a text, an integer
/// in decimal, each item of a list.
pub fn value_texts(value: &Value) -> Vec<String> {
    match value {
        Value::Texts(list) => list.iter().map(|t| t.to_string()).collect(),
        Value::Ints(list) => list.iter().map(i64::to_string).collect(),
        Value::Text(text) => vec![text.to_string()],
        other => vec![other.to_string()],
    }
}

/// The URL of `endpoint` over HTTPS with `path` in place of its own path:
/// `wss` is `https` for a GET.
pub fn https_url(endpoint: &Endpoint, path: Option<&str>) -> String {
    let host =
        if endpoint.host().contains(':') { format!("[{}]", endpoint.host()) } else { endpoint.host().to_string() };
    let port = if endpoint.port() == 443 { String::new() } else { format!(":{}", endpoint.port()) };
    let path = path.or(endpoint.path()).unwrap_or("/");
    format!("https://{host}{port}{path}")
}

/// The name of a fact for a line: `"NAME"`.
pub fn named(fact: &Fact) -> String {
    format!("{:?}", fact.name())
}

/// The bytes as lower-case hex.
pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
