//! The schema of a service file and its check (README, "The schema").
//! `build.rs` reads each built-in file through it, and the overlay (feature
//! `overlay`) reads the text of a consumer through it, so both apply the
//! same rules. It knows no provider and no kind: a kind is a word.

use std::fmt;
use std::net::IpAddr;

use toml::{Table, Value as Toml};

/// The words of the key `type` of a fact.
pub const TYPES: [&str; 6] = ["text", "int", "bool", "date", "texts", "ints"];

/// A fault of a file: the file, the field and why. A build fails with it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fault {
    pub file: String,
    pub field: String,
    pub why: String,
}

impl fmt::Display for Fault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}: {}", self.file, self.field, self.why)
    }
}

/// A day of the calendar, as TOML writes a local date.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Day {
    pub year: u16,
    pub month: u8,
    pub day: u8,
}

/// Where a value comes from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OriginSpec {
    /// A document.
    Url(String),
    /// A measurement, and its method.
    Measured(String),
    /// A ruling of an owner or an operator, and where it is written.
    Ruling(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceSpec {
    pub origin: OriginSpec,
    pub read: Day,
    pub note: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ValueSpec {
    Text(String),
    Int(i64),
    Bool(bool),
    Date(Day),
    Texts(Vec<String>),
    Ints(Vec<i64>),
    /// The text of a file under `services/` (built-in files only).
    File(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EndpointSpec {
    pub name: String,
    pub role: Option<String>,
    pub scheme: String,
    pub host: String,
    pub port: u16,
    pub path: Option<String>,
    pub addresses: Vec<String>,
    pub sni: Option<String>,
    pub alpn: Vec<String>,
    pub source: SourceSpec,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FactSpec {
    pub name: String,
    pub value: ValueSpec,
    pub unit: Option<String>,
    pub source: SourceSpec,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WatchSpec {
    pub kind: String,
    pub args: Vec<(String, ValueSpec)>,
}

/// One service, as its file gives it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Spec {
    pub id: String,
    pub kind: String,
    pub party: String,
    pub about: String,
    pub rank: Option<(u32, SourceSpec)>,
    pub endpoints: Vec<EndpointSpec>,
    pub facts: Vec<FactSpec>,
    pub watches: Vec<WatchSpec>,
}

/// A word: a lower-case letter first, then lower-case letters, digits and
/// single `-`, at most 64 bytes, no `-` at the end.
pub fn is_word(s: &str) -> bool {
    s.len() <= 64
        && s.starts_with(|c: char| c.is_ascii_lowercase())
        && !s.ends_with('-')
        && !s.contains("--")
        && s.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// A host: an IP literal, or letters, digits, `.`, `-` and `_`, not first
/// `-` or `.`, at most 253 bytes (a last `.` is a name that is fully
/// qualified).
pub fn is_host(s: &str) -> bool {
    if s.parse::<IpAddr>().is_ok() {
        return true;
    }
    !s.is_empty()
        && s.len() <= 253
        && !s.starts_with(['-', '.'])
        && !s.contains("..")
        && s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
}

/// A URI scheme (RFC 3986, 3.1), in lower case.
fn is_scheme(s: &str) -> bool {
    s.starts_with(|c: char| c.is_ascii_lowercase())
        && s.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '+' | '-' | '.'))
}

/// A path under `services/`: words of letters, digits, `.`, `-` and `_`,
/// separated by `/`, with no `..`.
fn is_file_path(s: &str) -> bool {
    !s.is_empty()
        && s.split('/').all(|part| {
            !part.is_empty()
                && part != "."
                && part != ".."
                && part.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
        })
}

/// One line of text: no control character, not empty.
fn is_line(s: &str) -> bool {
    !s.trim().is_empty() && !s.chars().any(char::is_control)
}

/// The text of a value: lines and tabs, no other control character.
fn is_text(s: &str) -> bool {
    !s.chars().any(|c| c.is_control() && c != '\n' && c != '\t')
}

/// The reader of one file: it names the file and the field of each fault.
struct Reader<'a> {
    file: &'a str,
}

impl Reader<'_> {
    fn fault(&self, field: &str, why: impl Into<String>) -> Fault {
        Fault { file: self.file.to_string(), field: field.to_string(), why: why.into() }
    }

    /// Refuse each key that `known` does not name: a mistyped key is a fault,
    /// not a value that nobody reads.
    fn keys(&self, field: &str, table: &Table, known: &[&str]) -> Result<(), Fault> {
        match table.keys().find(|k| !known.contains(&k.as_str())) {
            Some(k) => Err(self.fault(field, format!("the key {k:?} is not one of {}", known.join(", ")))),
            None => Ok(()),
        }
    }

    fn table<'t>(&self, field: &str, value: &'t Toml) -> Result<&'t Table, Fault> {
        value.as_table().ok_or_else(|| self.fault(field, "not a table"))
    }

    fn text<'t>(&self, field: &str, table: &'t Table, key: &str) -> Result<Option<&'t str>, Fault> {
        match table.get(key) {
            None => Ok(None),
            Some(Toml::String(s)) => Ok(Some(s)),
            Some(_) => Err(self.fault(&format!("{field}.{key}"), "not a text")),
        }
    }

    fn need<'t>(&self, field: &str, table: &'t Table, key: &str) -> Result<&'t str, Fault> {
        self.text(field, table, key)?.ok_or_else(|| self.fault(&format!("{field}.{key}"), "missing"))
    }

    fn word(&self, field: &str, table: &Table, key: &str) -> Result<String, Fault> {
        let s = self.need(field, table, key)?;
        if !is_word(s) {
            return Err(self.fault(&format!("{field}.{key}"), format!("{s:?} is not a word (a-z, 0-9, -)")));
        }
        Ok(s.to_string())
    }

    fn line(&self, field: &str, table: &Table, key: &str) -> Result<Option<String>, Fault> {
        match self.text(field, table, key)? {
            Some(s) if !is_line(s) => Err(self.fault(&format!("{field}.{key}"), "not one line of text")),
            other => Ok(other.map(str::to_string)),
        }
    }

    fn day(&self, field: &str, value: &Toml) -> Result<Day, Fault> {
        let Toml::Datetime(dt) = value else {
            return Err(self.fault(field, "not a date (YYYY-MM-DD, with no quotes)"));
        };
        match (dt.date, dt.time, dt.offset) {
            (Some(d), None, None) if (2000..=2100).contains(&d.year) => {
                Ok(Day { year: d.year, month: d.month, day: d.day })
            }
            _ => Err(self.fault(field, "not a date of this century (YYYY-MM-DD, with no time)")),
        }
    }

    /// The source of a value: exactly one of `url`, `measured` and `ruling`,
    /// the date `read`, and a `note` if any.
    fn source(&self, field: &str, owner: &Table) -> Result<SourceSpec, Fault> {
        let Some(value) = owner.get("source") else {
            return Err(self.fault(field, "no source (a table with url, measured or ruling, and read)"));
        };
        let field = format!("{field}.source");
        let table = self.table(&field, value)?;
        self.keys(&field, table, &["url", "measured", "ruling", "read", "note"])?;
        let mut origins = Vec::new();
        for key in ["url", "measured", "ruling"] {
            if let Some(text) = self.line(&field, table, key)? {
                origins.push((key, text));
            }
        }
        let origin = match origins.as_slice() {
            [("url", u)] if u.starts_with("https://") || u.starts_with("http://") => OriginSpec::Url(u.clone()),
            [("url", _)] => return Err(self.fault(&format!("{field}.url"), "not an https:// or http:// URL")),
            [("measured", m)] => OriginSpec::Measured(m.clone()),
            [("ruling", r)] => OriginSpec::Ruling(r.clone()),
            [] => return Err(self.fault(&field, "no url, measured or ruling")),
            _ => return Err(self.fault(&field, "more than one of url, measured and ruling")),
        };
        let read = match table.get("read") {
            Some(v) => self.day(&format!("{field}.read"), v)?,
            None => return Err(self.fault(&format!("{field}.read"), "missing: the date when the value was read")),
        };
        Ok(SourceSpec { origin, read, note: self.line(&field, table, "note")? })
    }

    /// A value of a fact or of a watch: a text, an integer, a bool, a date,
    /// or a list of texts or of integers.
    fn value(&self, field: &str, value: &Toml, kind: Option<&str>) -> Result<ValueSpec, Fault> {
        let value = match value {
            Toml::String(s) if is_text(s) => ValueSpec::Text(s.clone()),
            Toml::String(_) => return Err(self.fault(field, "a text with a control character")),
            Toml::Integer(i) => ValueSpec::Int(*i),
            Toml::Boolean(b) => ValueSpec::Bool(*b),
            Toml::Datetime(_) => ValueSpec::Date(self.day(field, value)?),
            Toml::Array(items) if items.is_empty() => match kind {
                Some("texts") => ValueSpec::Texts(Vec::new()),
                Some("ints") => ValueSpec::Ints(Vec::new()),
                _ => return Err(self.fault(field, "an empty list needs type = \"texts\" or \"ints\"")),
            },
            Toml::Array(items) => {
                if let Some(texts) = items.iter().map(|v| v.as_str().filter(|s| is_text(s))).collect::<Option<Vec<_>>>()
                {
                    ValueSpec::Texts(texts.into_iter().map(str::to_string).collect())
                } else if let Some(ints) = items.iter().map(Toml::as_integer).collect::<Option<Vec<_>>>() {
                    ValueSpec::Ints(ints)
                } else {
                    return Err(self.fault(field, "a list holds texts alone or integers alone"));
                }
            }
            _ => return Err(self.fault(field, "a value is a text, an integer, a bool, a date or a list")),
        };
        if let Some(kind) = kind
            && type_name(&value) != kind
        {
            return Err(
                self.fault(field, format!("the type is {kind}, and the value is of type {}", type_name(&value)))
            );
        }
        Ok(value)
    }

    fn list(&self, field: &str, table: &Table, key: &str) -> Result<Vec<String>, Fault> {
        match table.get(key) {
            None => Ok(Vec::new()),
            Some(Toml::Array(items)) => items
                .iter()
                .map(|v| {
                    v.as_str().map(str::to_string).ok_or_else(|| self.fault(&format!("{field}.{key}"), "not texts"))
                })
                .collect(),
            Some(_) => Err(self.fault(&format!("{field}.{key}"), "not a list of texts")),
        }
    }

    fn endpoint(&self, at: usize, table: &Table) -> Result<EndpointSpec, Fault> {
        const KEYS: [&str; 10] =
            ["name", "role", "scheme", "host", "port", "path", "addresses", "sni", "alpn", "source"];
        let field = format!("endpoint {at}");
        self.keys(&field, table, &KEYS)?;
        let name = self.word(&field, table, "name")?;
        let field = format!("endpoint {name:?}");
        let role = match table.get("role") {
            Some(_) => Some(self.word(&field, table, "role")?),
            None => None,
        };
        let scheme = self.need(&field, table, "scheme")?;
        if !is_scheme(scheme) {
            return Err(self.fault(&format!("{field}.scheme"), format!("{scheme:?} is not a scheme in lower case")));
        }
        let host = self.need(&field, table, "host")?;
        if !is_host(host) {
            return Err(self.fault(&format!("{field}.host"), format!("{host:?} is not a host name or an address")));
        }
        let port = match table.get("port") {
            Some(Toml::Integer(p)) if (1..=65535).contains(p) => *p as u16,
            Some(_) => return Err(self.fault(&format!("{field}.port"), "not a port (1 to 65535)")),
            None => return Err(self.fault(&format!("{field}.port"), "missing")),
        };
        let path = self.line(&field, table, "path")?;
        if path.as_deref().is_some_and(|p| !p.starts_with('/') || p.contains(' ')) {
            return Err(self.fault(&format!("{field}.path"), "a path starts with / and has no space"));
        }
        let addresses = self.list(&field, table, "addresses")?;
        if let Some(bad) = addresses.iter().find(|a| a.parse::<IpAddr>().is_err()) {
            return Err(self.fault(&format!("{field}.addresses"), format!("{bad:?} is not an IP address")));
        }
        let sni = self.line(&field, table, "sni")?;
        if sni.as_deref().is_some_and(|s| !is_host(s) || s.parse::<IpAddr>().is_ok()) {
            return Err(self.fault(&format!("{field}.sni"), "not a host name"));
        }
        let alpn = self.list(&field, table, "alpn")?;
        if alpn.iter().any(|a| a.is_empty() || a.len() > 255 || !is_line(a)) {
            return Err(self.fault(&format!("{field}.alpn"), "an ALPN word is 1 to 255 bytes of one line"));
        }
        let source = self.source(&field, table)?;
        let (scheme, host) = (scheme.to_string(), host.to_string());
        Ok(EndpointSpec { name, role, scheme, host, port, path, addresses, sni, alpn, source })
    }

    fn fact(&self, at: usize, table: &Table, files: bool) -> Result<FactSpec, Fault> {
        let field = format!("fact {at}");
        self.keys(&field, table, &["name", "value", "file", "type", "unit", "source"])?;
        let name = self.word(&field, table, "name")?;
        let field = format!("fact {name:?}");
        let kind = self.text(&field, table, "type")?;
        if let Some(kind) = kind
            && !TYPES.contains(&kind)
        {
            return Err(self.fault(&format!("{field}.type"), format!("{kind:?} is not one of {}", TYPES.join(", "))));
        }
        let value = match (table.get("value"), self.text(&field, table, "file")?) {
            (Some(v), None) => self.value(&format!("{field}.value"), v, kind)?,
            (None, Some(_)) if !files => {
                return Err(self.fault(&format!("{field}.file"), "an overlay gives the text in value, not a file"));
            }
            (None, Some(path)) if is_file_path(path) && kind.is_none_or(|k| k == "text") => {
                ValueSpec::File(path.to_string())
            }
            (None, Some(_)) => return Err(self.fault(&format!("{field}.file"), "not a path of a text file")),
            (Some(_), Some(_)) => return Err(self.fault(&field, "both a value and a file")),
            (None, None) => return Err(self.fault(&field, "no value")),
        };
        let unit = match table.get("unit") {
            Some(_) => Some(self.word(&field, table, "unit")?),
            None => None,
        };
        let source = self.source(&field, table)?;
        Ok(FactSpec { name, value, unit, source })
    }

    fn watch(&self, at: usize, table: &Table) -> Result<WatchSpec, Fault> {
        let field = format!("watch {at}");
        let kind = self.word(&field, table, "kind")?;
        let mut args = Vec::new();
        for (key, value) in table.iter().filter(|(k, _)| k.as_str() != "kind") {
            if !is_word(key) {
                return Err(self.fault(&format!("{field}.{key}"), "an argument's name is a word"));
            }
            args.push((key.clone(), self.value(&format!("{field}.{key}"), value, None)?));
        }
        Ok(WatchSpec { kind, args })
    }

    fn tables<'t>(&self, top: &'t Table, key: &str) -> Result<Vec<&'t Table>, Fault> {
        match top.get(key) {
            None => Ok(Vec::new()),
            Some(Toml::Array(items)) => items.iter().map(|v| self.table(key, v)).collect(),
            Some(_) => Err(self.fault(key, format!("not an array of tables ([[{key}]])"))),
        }
    }
}

/// The type of a value, as the key `type` of a fact names it.
pub fn type_name(value: &ValueSpec) -> &'static str {
    match value {
        ValueSpec::Text(_) | ValueSpec::File(_) => "text",
        ValueSpec::Int(_) => "int",
        ValueSpec::Bool(_) => "bool",
        ValueSpec::Date(_) => "date",
        ValueSpec::Texts(_) => "texts",
        ValueSpec::Ints(_) => "ints",
    }
}

/// Read and check the text of one service file. `file` names it in each
/// fault. `files` allows a fact to name a file under `services/` (the
/// built-in set); an overlay gives each text in its value.
pub fn parse(file: &str, text: &str, files: bool) -> Result<Spec, Fault> {
    let r = Reader { file };
    let top: Table = text.parse().map_err(|e: toml::de::Error| r.fault("TOML", e.message().to_string()))?;
    r.keys("top", &top, &["id", "kind", "party", "about", "rank", "endpoint", "fact", "watch"])?;
    let id = r.word("top", &top, "id")?;
    let kind = r.word("top", &top, "kind")?;
    let party = r.line("top", &top, "party")?.ok_or_else(|| r.fault("party", "missing"))?;
    let about = r.line("top", &top, "about")?.ok_or_else(|| r.fault("about", "missing"))?;
    let rank = match top.get("rank") {
        None => None,
        Some(value) => {
            let table = r.table("rank", value)?;
            r.keys("rank", table, &["value", "source"])?;
            let value = match table.get("value").and_then(Toml::as_integer).map(u32::try_from) {
                Some(Ok(v)) => v,
                _ => return Err(r.fault("rank.value", "not a whole number from 0")),
            };
            Some((value, r.source("rank", table)?))
        }
    };
    let endpoints =
        r.tables(&top, "endpoint")?.iter().enumerate().map(|(i, t)| r.endpoint(i, t)).collect::<Result<Vec<_>, _>>()?;
    let facts =
        r.tables(&top, "fact")?.iter().enumerate().map(|(i, t)| r.fact(i, t, files)).collect::<Result<Vec<_>, _>>()?;
    let watches =
        r.tables(&top, "watch")?.iter().enumerate().map(|(i, t)| r.watch(i, t)).collect::<Result<Vec<_>, _>>()?;
    if endpoints.is_empty() && facts.is_empty() {
        return Err(r.fault("top", "no endpoint and no fact"));
    }
    if watches.is_empty() {
        return Err(r.fault("watch", "no watch: each service names at least one [[watch]]"));
    }
    if let Some(name) = duplicate(endpoints.iter().map(|e| e.name.as_str())) {
        return Err(r.fault(&format!("endpoint {name:?}"), "a second endpoint of this name"));
    }
    if let Some(name) = duplicate(facts.iter().map(|f| f.name.as_str())) {
        return Err(r.fault(&format!("fact {name:?}"), "a second fact of this name"));
    }
    Ok(Spec { id, kind, party, about, rank, endpoints, facts, watches })
}

/// The first name that comes twice.
pub fn duplicate<'a>(names: impl Iterator<Item = &'a str>) -> Option<&'a str> {
    let mut seen = std::collections::BTreeSet::new();
    names.into_iter().find(|n| !seen.insert(*n))
}

/// Check a set of files: each id once, and each file named for its id
/// (`ID.toml`) when `named` is true.
pub fn check_set<'a>(specs: impl Iterator<Item = (&'a str, &'a Spec)>, named: bool) -> Result<(), Fault> {
    let mut seen = std::collections::BTreeMap::new();
    for (file, spec) in specs {
        let stem = file.rsplit('/').next().unwrap_or(file).strip_suffix(".toml").unwrap_or(file);
        if named && stem != spec.id {
            let why = format!("the id {:?} is not the name of its file ({stem}.toml)", spec.id);
            return Err(Fault { file: file.to_string(), field: "id".into(), why });
        }
        if let Some(first) = seen.insert(spec.id.clone(), file.to_string()) {
            let why = format!("the id {:?} is the id of {first} too", spec.id);
            return Err(Fault { file: file.to_string(), field: "id".into(), why });
        }
    }
    Ok(())
}
