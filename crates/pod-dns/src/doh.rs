//! The resolvers of DNS over HTTPS, as data: each one an address (a
//! resolver needs no resolver), the name that its certificate names, its
//! port, the path of each form that it serves, and whether it serves HTTP/2
//! alone. The crate holds no list of hosts: the default list comes from the
//! services of the kind `doh` of `pod-services` ([`defaults`]), and a
//! consumer gives its own in [`crate::Settings::resolvers`].

use std::net::IpAddr;

use pod_services::Service;

use crate::wire;

/// A resolver of DNS over HTTPS, asked by its address, or by its name
/// through a proxy that resolves names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Doh {
    /// Who runs it: [`crate::Nxdomain::NextOperator`] passes over the other
    /// addresses of an operator that says that a name does not exist.
    pub operator: String,
    /// Its address, which its certificate names.
    pub ip: IpAddr,
    /// Its name, which its certificate names too: through a proxy it is asked
    /// by it, as a proxy that takes names alone refuses an address.
    pub name: String,
    /// The port of its HTTPS (443).
    pub port: u16,
    /// The path of its JSON form (`?name=...&type=...`), where it serves one.
    pub json: Option<String>,
    /// The path of RFC 8484's wire form (`?dns=...`), where it serves one.
    pub wire: Option<String>,
    /// Whether it serves HTTP/2 alone, so that it is asked over HTTP/2
    /// ([`crate::h2`]): Quad9 answered HTTP/1.1 with `505`.
    pub http2: bool,
}

/// How a resolver is asked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Form {
    /// Its JSON form, `application/dns-json`, which no standard defines.
    Json,
    /// RFC 8484's GET with `?dns=`, `application/dns-message`.
    Wire,
}

impl Form {
    /// The form in a few words, as a doctor names it.
    pub fn words(self) -> &'static str {
        match self {
            Form::Json => "its JSON API",
            Form::Wire => "RFC 8484's wire form",
        }
    }

    /// The type of the answer that the form asks for.
    pub fn accept(self) -> &'static str {
        match self {
            Form::Json => "application/dns-json",
            Form::Wire => "application/dns-message",
        }
    }
}

/// Where the names of the resolvers go in the order, through a way whose far
/// end resolves names ([`pod_core::Dial::names_inside`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Names {
    /// Each resolver by its name, then by its address (podssh): a proxy that
    /// takes names alone refuses an address, and one that takes addresses
    /// alone refuses the name at once.
    #[default]
    First,
    /// Each resolver by its address, then each name (podnet).
    Last,
}

impl Doh {
    /// A resolver at `ip` whose certificate names `name`, on port 443, with
    /// no form yet; its operator is its name.
    pub fn new(ip: IpAddr, name: impl Into<String>) -> Doh {
        let name = name.into();
        Doh { operator: name.clone(), ip, name, port: 443, json: None, wire: None, http2: false }
    }

    /// The same resolver, of `operator`.
    pub fn operator(mut self, operator: impl Into<String>) -> Doh {
        self.operator = operator.into();
        self
    }

    /// The same resolver, at `port`.
    pub fn port(mut self, port: u16) -> Doh {
        self.port = port;
        self
    }

    /// The same resolver, with the JSON form at `path`.
    pub fn json(mut self, path: impl Into<String>) -> Doh {
        self.json = Some(path.into());
        self
    }

    /// The same resolver, with the wire form at `path`.
    pub fn wire(mut self, path: impl Into<String>) -> Doh {
        self.wire = Some(path.into());
        self
    }

    /// The same resolver, over HTTP/2 alone when `http2`.
    pub fn http2(mut self, http2: bool) -> Doh {
        self.http2 = http2;
        self
    }

    /// The resolvers of one service of the kind `doh` (`pod-services`): one
    /// for each address of each endpoint of `roles`, in the order of the
    /// file. Its party is the operator, the facts `json-path`, `wire-path`
    /// and `http2-only` are the forms.
    pub fn of_service(service: &Service, roles: &[&str]) -> Vec<Doh> {
        let mut out = Vec::new();
        for endpoint in service.endpoints() {
            if !endpoint.role().is_some_and(|role| roles.contains(&role)) {
                continue;
            }
            for ip in endpoint.ip_addrs() {
                out.push(Doh {
                    operator: service.party().to_string(),
                    ip,
                    name: endpoint.host().trim_end_matches('.').to_string(),
                    port: endpoint.port(),
                    json: service.text("json-path").map(str::to_string),
                    wire: service.text("wire-path").map(str::to_string),
                    http2: service.bool("http2-only").unwrap_or(false),
                });
            }
        }
        out
    }

    /// The forms to ask, in order: the JSON form first where there is one,
    /// then the wire form, its fallback and the only form of a resolver with
    /// no JSON.
    pub fn forms(&self) -> Vec<Form> {
        let json = self.json.as_ref().map(|_| Form::Json);
        let wire = self.wire.as_ref().map(|_| Form::Wire);
        json.into_iter().chain(wire).collect()
    }

    /// The request target of the question of `kind` for `name` in `form`; a
    /// resolver with one form gives that form.
    pub fn target(&self, form: Form, name: &str, kind: &str) -> Result<String, String> {
        match (form, &self.json, &self.wire) {
            (Form::Json, Some(path), _) | (Form::Wire, Some(path), None) => {
                Ok(format!("{path}?name={}&type={kind}", encode(name)))
            }
            (_, _, Some(path)) => {
                let code = wire::code(kind).ok_or_else(|| format!("no record type {kind}"))?;
                Ok(format!("{path}?dns={}", wire::query_param(name, code)?))
            }
            (_, None, None) => Err(format!("{} serves no form", self.ip)),
        }
    }

    /// Where it is asked, in order. Through a way whose far end resolves
    /// names (`by_name`), by its name first; with none, by its address alone:
    /// a name would need the resolver that this stands in for. A name already
    /// in `asked`, which two addresses of an operator share, is not asked
    /// again.
    pub fn hosts(&self, by_name: bool, asked: &[String]) -> Vec<String> {
        let by_name = by_name && !asked.contains(&self.name);
        by_name.then(|| self.name.clone()).into_iter().chain([self.ip.to_string()]).collect()
    }
}

/// `name` with each byte outside the unreserved set of RFC 3986 as `%XX`.
fn encode(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for b in name.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// The hosts to ask of `resolvers`, in order, each with the index of its
/// resolver: by [`Names`], with the names only when `by_name`, and each name
/// once.
pub fn order(resolvers: &[Doh], names: Names, by_name: bool) -> Vec<(usize, String)> {
    let mut out: Vec<(usize, String)> = Vec::new();
    let mut asked: Vec<String> = Vec::new();
    match names {
        Names::First => {
            for (i, resolver) in resolvers.iter().enumerate() {
                for host in resolver.hosts(by_name, &asked) {
                    asked.push(host.clone());
                    out.push((i, host));
                }
            }
        }
        Names::Last => {
            out.extend(resolvers.iter().enumerate().map(|(i, r)| (i, r.ip.to_string())));
            for (i, resolver) in resolvers.iter().enumerate() {
                if by_name && !asked.contains(&resolver.name) {
                    asked.push(resolver.name.clone());
                    out.push((i, resolver.name.clone()));
                }
            }
        }
    }
    out
}

/// The resolvers of `services`, in their order, each by
/// [`Doh::of_service`] with `roles`.
pub fn of_services<'a>(services: impl IntoIterator<Item = &'a Service>, roles: &[&str]) -> Vec<Doh> {
    services.into_iter().flat_map(|service| Doh::of_service(service, roles)).collect()
}

/// podssh's order: the operators take turns, the first address of each,
/// then the second, in the order of their first resolver; the operators
/// that serve the JSON form first, then those that serve the wire form
/// alone (Quad9: a GET of its JSON form got 400).
pub fn turns(list: Vec<Doh>) -> Vec<Doh> {
    let mut operators: Vec<(String, Vec<Doh>)> = Vec::new();
    for resolver in list {
        match operators.iter_mut().find(|(operator, _)| *operator == resolver.operator) {
            Some((_, own)) => own.push(resolver),
            None => operators.push((resolver.operator.clone(), vec![resolver])),
        }
    }
    let (json, rest): (Vec<_>, Vec<_>) =
        operators.into_iter().map(|(_, own)| own).partition(|own| own.iter().any(|r| r.json.is_some()));
    [json, rest].into_iter().flat_map(round_robin).collect()
}

/// One of each group in turn, until each group is empty.
fn round_robin(groups: Vec<Vec<Doh>>) -> Vec<Doh> {
    let mut iters: Vec<_> = groups.into_iter().map(Vec::into_iter).collect();
    let mut out = Vec::new();
    loop {
        let before = out.len();
        out.extend(iters.iter_mut().filter_map(Iterator::next));
        if out.len() == before {
            return out;
        }
    }
}

/// The default resolvers: the IPv4 addresses (role `v4`) of the services of
/// the kind `doh` of `pod-services`, in podssh's [`turns`].
pub fn defaults() -> Vec<Doh> {
    turns(of_services(pod_services::of_kind("doh"), &["v4"]))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// podnet's order: the servers by address first, in their order; the
    /// servers by name after them, and only through a way whose far end
    /// resolves names (a proxy).
    #[test]
    fn the_servers_by_name_need_a_proxy() {
        let services = ["cloudflare-doh", "google-doh"].map(|id| pod_services::get(id).expect(id));
        let servers = of_services(services, &["v4", "v6"]);
        let direct = order(&servers, Names::Last, false);
        let addresses: Vec<String> = servers.iter().map(|s| s.ip.to_string()).collect();
        assert_eq!(direct.iter().map(|(_, host)| host.clone()).collect::<Vec<_>>(), addresses);
        let proxied = order(&servers, Names::Last, true);
        assert_eq!(proxied[..servers.len()], direct[..]);
        let names: Vec<&str> = proxied[servers.len()..].iter().map(|(_, host)| host.as_str()).collect();
        assert_eq!(names, ["cloudflare-dns.com", "dns.google", "dns.google.com"]);
        for (i, name) in &proxied[servers.len()..] {
            assert_eq!(servers[*i].name, *name, "each name with its resolver");
        }
    }

    #[test]
    fn the_json_name_is_percent_encoded() {
        let doh = Doh::new([192, 0, 2, 1].into(), "doh.test").json("/q");
        assert_eq!(doh.target(Form::Json, "a-b_c.example", "A").unwrap(), "/q?name=a-b_c.example&type=A");
        assert_eq!(doh.target(Form::Json, "a&b#x%y z", "A").unwrap(), "/q?name=a%26b%23x%25y%20z&type=A");
    }

    /// podssh's order of the default resolvers: Cloudflare's and Google's
    /// addresses take turns, then Quad9's, which serve the wire form alone.
    #[test]
    fn the_default_resolvers_take_turns_and_the_wire_form_alone_comes_last() {
        let ips: Vec<String> = defaults().iter().map(|r| r.ip.to_string()).collect();
        assert_eq!(ips, ["1.1.1.1", "8.8.8.8", "1.0.0.1", "8.8.4.4", "9.9.9.9", "149.112.112.112"]);
        let names: Vec<String> = defaults().into_iter().map(|r| r.name).collect();
        let want = ["cloudflare-dns.com", "dns.google", "cloudflare-dns.com", "dns.google.com", "dns.quad9.net"];
        assert_eq!(names[..5], want);
        assert_eq!(names[5], "dns9.quad9.net");
    }
}
