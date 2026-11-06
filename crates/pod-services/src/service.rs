//! A service and its values: the endpoints, the facts and the watches, each
//! value with its source and the date when it was read. The built-in set
//! borrows each text from the binary; a service of an overlay owns its
//! texts. The types are the same, so a consumer reads both alike.

use std::borrow::Cow;
use std::fmt;
use std::net::{IpAddr, SocketAddr};

/// A text: borrowed from the binary, or owned by an overlay.
pub type Text = Cow<'static, str>;
/// A list: borrowed from the binary, or owned by an overlay.
pub type List<T> = Cow<'static, [T]>;

/// One service of another party: its id, its kind, its endpoints, its facts
/// and its watches.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Service {
    pub(crate) id: Text,
    pub(crate) kind: Text,
    pub(crate) party: Text,
    pub(crate) about: Text,
    pub(crate) rank: Option<Rank>,
    pub(crate) endpoints: List<Endpoint>,
    pub(crate) facts: List<Fact>,
    pub(crate) watches: List<Watch>,
}

/// The place of a service in the lists of its kind, and its source.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rank {
    pub(crate) value: u32,
    pub(crate) source: Source,
}

/// Where a consumer connects: a scheme, a host and a port, and what the
/// connection needs (a path, addresses, a TLS name, ALPN).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Endpoint {
    pub(crate) name: Text,
    pub(crate) role: Option<Text>,
    pub(crate) scheme: Text,
    pub(crate) host: Text,
    pub(crate) port: u16,
    pub(crate) path: Option<Text>,
    pub(crate) addresses: List<Text>,
    pub(crate) sni: Option<Text>,
    pub(crate) alpn: List<Text>,
    pub(crate) source: Source,
}

/// A fact of a service: a key, a pin, a limit, a version, a path, a text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fact {
    pub(crate) name: Text,
    pub(crate) value: Value,
    pub(crate) unit: Option<Text>,
    pub(crate) source: Source,
}

/// A typed value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Value {
    Text(Text),
    Int(i64),
    Bool(bool),
    Date(Date),
    Texts(List<Text>),
    Ints(List<i64>),
}

/// A day of the calendar.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Date {
    pub(crate) year: u16,
    pub(crate) month: u8,
    pub(crate) day: u8,
}

/// The source of a value and the date when it was read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Source {
    pub(crate) origin: Origin,
    pub(crate) read: Date,
    pub(crate) note: Option<Text>,
}

/// Where a value comes from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Origin {
    /// A document.
    Url(Text),
    /// A measurement, with its method.
    Measured(Text),
    /// A ruling, and where it is written.
    Ruling(Text),
}

/// A watch of a service: a kind and its arguments. The workflows of
/// podcrates run each kind (`pod-services-watch`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Watch {
    pub(crate) kind: Text,
    pub(crate) args: List<Arg>,
}

/// One argument of a watch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Arg {
    pub(crate) name: Text,
    pub(crate) value: Value,
}

impl Service {
    pub fn id(&self) -> &str {
        &self.id
    }

    /// The kind: a word, as `relay`, `doh`, `stun`.
    pub fn kind(&self) -> &str {
        &self.kind
    }

    /// Who runs the service.
    pub fn party(&self) -> &str {
        &self.party
    }

    /// What the service is, in one line.
    pub fn about(&self) -> &str {
        &self.about
    }

    /// The place in the lists of its kind: a lower rank first; none after
    /// each ranked service.
    pub fn rank(&self) -> Option<&Rank> {
        self.rank.as_ref()
    }

    pub fn endpoints(&self) -> &[Endpoint] {
        &self.endpoints
    }

    /// The endpoint of `name`.
    pub fn endpoint(&self, name: &str) -> Option<&Endpoint> {
        self.endpoints.iter().find(|e| e.name == name)
    }

    /// The endpoints of the role `role`, in the order of the file.
    pub fn endpoints_of<'a>(&'a self, role: &'a str) -> impl Iterator<Item = &'a Endpoint> + 'a {
        self.endpoints.iter().filter(move |e| e.role.as_deref() == Some(role))
    }

    pub fn facts(&self) -> &[Fact] {
        &self.facts
    }

    /// The fact of `name`.
    pub fn fact(&self, name: &str) -> Option<&Fact> {
        self.facts.iter().find(|f| f.name == name)
    }

    /// The text of the fact `name`; none when it is missing or of another type.
    pub fn text(&self, name: &str) -> Option<&str> {
        self.fact(name)?.value.as_text()
    }

    pub fn int(&self, name: &str) -> Option<i64> {
        self.fact(name)?.value.as_int()
    }

    pub fn bool(&self, name: &str) -> Option<bool> {
        self.fact(name)?.value.as_bool()
    }

    pub fn date(&self, name: &str) -> Option<Date> {
        self.fact(name)?.value.as_date()
    }

    pub fn texts(&self, name: &str) -> Option<&[Text]> {
        self.fact(name)?.value.as_texts()
    }

    pub fn ints(&self, name: &str) -> Option<&[i64]> {
        self.fact(name)?.value.as_ints()
    }

    pub fn watches(&self) -> &[Watch] {
        &self.watches
    }

    /// The key of the order of a kind: the rank, then the id.
    pub(crate) fn order(&self) -> (u32, &str) {
        (self.rank.as_ref().map_or(u32::MAX, |r| r.value), &self.id)
    }
}

impl Rank {
    pub fn value(&self) -> u32 {
        self.value
    }

    pub fn source(&self) -> &Source {
        &self.source
    }
}

impl Endpoint {
    /// The name of the endpoint in its service, a word.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The role, a word that groups endpoints (`v4`, `seed`, `region`).
    pub fn role(&self) -> Option<&str> {
        self.role.as_deref()
    }

    pub fn scheme(&self) -> &str {
        &self.scheme
    }

    /// The host: a name, or an IP address with no brackets.
    pub fn host(&self) -> &str {
        &self.host
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn path(&self) -> Option<&str> {
        self.path.as_deref()
    }

    /// The addresses of the host that the consumer dials with no resolver.
    pub fn addresses(&self) -> &[Text] {
        &self.addresses
    }

    /// The addresses, parsed (the build checks that each one parses).
    pub fn ip_addrs(&self) -> Vec<IpAddr> {
        self.addresses.iter().filter_map(|a| a.parse().ok()).collect()
    }

    /// The addresses with the port; the host, when it is an address and no
    /// address is given.
    pub fn socket_addrs(&self) -> Vec<SocketAddr> {
        let mut ips = self.ip_addrs();
        if ips.is_empty()
            && let Ok(ip) = self.host.parse::<IpAddr>()
        {
            ips.push(ip);
        }
        ips.into_iter().map(|ip| SocketAddr::new(ip, self.port)).collect()
    }

    /// The TLS name of the endpoint, when it differs from the host.
    pub fn sni(&self) -> Option<&str> {
        self.sni.as_deref()
    }

    /// The name for TLS: the SNI, else the host.
    pub fn server_name(&self) -> &str {
        self.sni.as_deref().unwrap_or(&self.host)
    }

    /// The ALPN words, in order; none for a connection with no ALPN.
    pub fn alpn(&self) -> &[Text] {
        &self.alpn
    }

    /// `host:port`, with brackets around an IPv6 host.
    pub fn authority(&self) -> String {
        if self.host.contains(':') {
            format!("[{}]:{}", self.host, self.port)
        } else {
            format!("{}:{}", self.host, self.port)
        }
    }

    /// `scheme://host[:port][path]`: the port is left out when it is the
    /// default of the scheme (443 for https and wss, 80 for http and ws).
    pub fn url(&self) -> String {
        let default = matches!((&*self.scheme, self.port), ("https" | "wss", 443) | ("http" | "ws", 80));
        let host = if self.host.contains(':') { format!("[{}]", self.host) } else { self.host.to_string() };
        let port = if default { String::new() } else { format!(":{}", self.port) };
        format!("{}://{host}{port}{}", self.scheme, self.path.as_deref().unwrap_or(""))
    }

    pub fn source(&self) -> &Source {
        &self.source
    }
}

impl Fact {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn value(&self) -> &Value {
        &self.value
    }

    /// The unit of a number (`ms`, `bytes`), a word.
    pub fn unit(&self) -> Option<&str> {
        self.unit.as_deref()
    }

    pub fn source(&self) -> &Source {
        &self.source
    }
}

impl Value {
    pub fn as_text(&self) -> Option<&str> {
        if let Value::Text(t) = self { Some(t) } else { None }
    }

    pub fn as_int(&self) -> Option<i64> {
        if let Value::Int(i) = self { Some(*i) } else { None }
    }

    pub fn as_bool(&self) -> Option<bool> {
        if let Value::Bool(b) = self { Some(*b) } else { None }
    }

    pub fn as_date(&self) -> Option<Date> {
        if let Value::Date(d) = self { Some(*d) } else { None }
    }

    pub fn as_texts(&self) -> Option<&[Text]> {
        if let Value::Texts(t) = self { Some(t) } else { None }
    }

    pub fn as_ints(&self) -> Option<&[i64]> {
        if let Value::Ints(i) = self { Some(i) } else { None }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Text(t) => f.write_str(t),
            Value::Int(i) => write!(f, "{i}"),
            Value::Bool(b) => write!(f, "{b}"),
            Value::Date(d) => write!(f, "{d}"),
            Value::Texts(t) => f.write_str(&t.join(", ")),
            Value::Ints(i) => f.write_str(&i.iter().map(i64::to_string).collect::<Vec<_>>().join(", ")),
        }
    }
}

impl Date {
    /// A day; none when it is not a day of the calendar.
    pub fn new(year: u16, month: u8, day: u8) -> Option<Date> {
        let leap = year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400));
        let days = match month {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            2 if leap => 29,
            2 => 28,
            _ => return None,
        };
        (1..=days).contains(&day).then_some(Date { year, month, day })
    }

    pub fn year(&self) -> u16 {
        self.year
    }

    pub fn month(&self) -> u8 {
        self.month
    }

    pub fn day(&self) -> u8 {
        self.day
    }
}

impl fmt::Display for Date {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

impl Source {
    pub fn origin(&self) -> &Origin {
        &self.origin
    }

    /// The date when the value was read at its source.
    pub fn read(&self) -> Date {
        self.read
    }

    pub fn note(&self) -> Option<&str> {
        self.note.as_deref()
    }
}

impl fmt::Display for Origin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Origin::Url(u) => write!(f, "url {u}"),
            Origin::Measured(m) => write!(f, "measured: {m}"),
            Origin::Ruling(r) => write!(f, "ruling: {r}"),
        }
    }
}

impl Watch {
    /// The kind of the watch, a word (`http-json`, `dns-srv`, `rdap`).
    pub fn kind(&self) -> &str {
        &self.kind
    }

    pub fn args(&self) -> &[Arg] {
        &self.args
    }

    /// The argument of `name`.
    pub fn arg(&self, name: &str) -> Option<&Value> {
        self.args.iter().find(|a| a.name == name).map(|a| &a.value)
    }
}

impl Arg {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn value(&self) -> &Value {
        &self.value
    }
}
