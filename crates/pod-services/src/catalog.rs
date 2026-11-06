//! The set of services of a consumer: the built-in set, and the files of
//! the consumer laid over it (feature `overlay`). The consumer reads its
//! files itself and gives their text: the crate reads no file, no variable
//! and no directory (docs/design.md, section 1, rule 6).

use crate::service::Service;

/// The built-in services, with the services of the consumer's overlay. A
/// service of the overlay replaces the built-in service of the same id; a
/// new id adds a service.
#[derive(Clone, Debug, Default)]
pub struct Catalog {
    overlay: Vec<Service>,
}

impl Catalog {
    /// The built-in set alone.
    pub fn new() -> Catalog {
        Catalog::default()
    }

    /// The service of `id`: the overlay's, else the built-in one.
    pub fn get(&self, id: &str) -> Option<&Service> {
        self.overlay.iter().find(|s| s.id() == id).or_else(|| crate::get(id))
    }

    /// The services of `kind`, by rank and then by id.
    pub fn of_kind<'a>(&'a self, kind: &'a str) -> impl Iterator<Item = &'a Service> + 'a {
        self.services().filter(move |s| s.kind() == kind).collect::<Vec<_>>().into_iter()
    }

    /// Each service, by rank and then by id.
    pub fn iter(&self) -> impl Iterator<Item = &Service> {
        self.services()
    }

    /// The services of the overlay, in the order that they were laid.
    pub fn overlay(&self) -> &[Service] {
        &self.overlay
    }

    fn services(&self) -> std::vec::IntoIter<&Service> {
        let mut all: Vec<&Service> = self.overlay.iter().collect();
        all.extend(crate::all().iter().filter(|s| !self.overlay.iter().any(|o| o.id() == s.id())));
        all.sort_by(|a, b| a.order().cmp(&b.order()));
        all.into_iter()
    }
}

#[cfg(feature = "overlay")]
mod overlay {
    use std::borrow::Cow;
    use std::fmt;

    use super::Catalog;
    use crate::schema::{self, Day, OriginSpec, SourceSpec, Spec, ValueSpec};
    use crate::service::{Arg, Date, Endpoint, Fact, Origin, Rank, Service, Source, Value, Watch};

    /// A file of an overlay that does not hold to the schema: the label that
    /// the consumer gave, the field and why.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct OverlayError {
        pub label: String,
        pub field: String,
        pub why: String,
    }

    impl OverlayError {
        /// A fault of the consumer's settings.
        pub fn class(&self) -> pod_core::Class {
            pod_core::Class::Config
        }
    }

    impl fmt::Display for OverlayError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            let line = |s: &str| pod_core::text::one_line(s);
            write!(f, "{}: {}: {}", line(&self.label), line(&self.field), line(&self.why))
        }
    }

    impl std::error::Error for OverlayError {}

    impl From<schema::Fault> for OverlayError {
        fn from(f: schema::Fault) -> OverlayError {
            OverlayError { label: f.file, field: f.field, why: f.why }
        }
    }

    impl Catalog {
        /// Lay the text of one file of the schema over the set. `label` names
        /// the file in each error (the consumer's path, for example). A fact
        /// gives its text in `value`: an overlay names no file. A second file
        /// of the overlay with the same id is an error; a file with the id of
        /// a built-in service replaces it.
        pub fn lay(&mut self, label: &str, text: &str) -> Result<&Service, OverlayError> {
            let spec = schema::parse(label, text, false)?;
            if self.overlay.iter().any(|s| s.id() == spec.id) {
                let why = format!("the id {:?} is laid already", spec.id);
                return Err(OverlayError { label: label.to_string(), field: "id".into(), why });
            }
            self.overlay.push(service(spec));
            Ok(self.overlay.last().expect("pushed"))
        }

        /// A catalog of the built-in set with each `(label, text)` laid over it.
        pub fn with_overlay<'a>(files: impl IntoIterator<Item = (&'a str, &'a str)>) -> Result<Catalog, OverlayError> {
            let mut catalog = Catalog::new();
            for (label, text) in files {
                catalog.lay(label, text)?;
            }
            Ok(catalog)
        }
    }

    fn date(d: Day) -> Date {
        Date { year: d.year, month: d.month, day: d.day }
    }

    fn source(s: SourceSpec) -> Source {
        let origin = match s.origin {
            OriginSpec::Url(u) => Origin::Url(u.into()),
            OriginSpec::Measured(m) => Origin::Measured(m.into()),
            OriginSpec::Ruling(r) => Origin::Ruling(r.into()),
        };
        Source { origin, read: date(s.read), note: s.note.map(Cow::Owned) }
    }

    fn value(v: ValueSpec) -> Value {
        match v {
            // The parse of an overlay refuses a file.
            ValueSpec::Text(t) | ValueSpec::File(t) => Value::Text(t.into()),
            ValueSpec::Int(i) => Value::Int(i),
            ValueSpec::Bool(b) => Value::Bool(b),
            ValueSpec::Date(d) => Value::Date(date(d)),
            ValueSpec::Texts(t) => Value::Texts(t.into_iter().map(Cow::Owned).collect::<Vec<_>>().into()),
            ValueSpec::Ints(i) => Value::Ints(i.into()),
        }
    }

    fn texts(t: Vec<String>) -> Cow<'static, [Cow<'static, str>]> {
        Cow::Owned(t.into_iter().map(Cow::Owned).collect())
    }

    fn service(spec: Spec) -> Service {
        let endpoints = spec
            .endpoints
            .into_iter()
            .map(|e| Endpoint {
                name: e.name.into(),
                role: e.role.map(Cow::Owned),
                scheme: e.scheme.into(),
                host: e.host.into(),
                port: e.port,
                path: e.path.map(Cow::Owned),
                addresses: texts(e.addresses),
                sni: e.sni.map(Cow::Owned),
                alpn: texts(e.alpn),
                source: source(e.source),
            })
            .collect::<Vec<_>>();
        let facts = spec
            .facts
            .into_iter()
            .map(|f| Fact {
                name: f.name.into(),
                value: value(f.value),
                unit: f.unit.map(Cow::Owned),
                source: source(f.source),
            })
            .collect::<Vec<_>>();
        let watches = spec
            .watches
            .into_iter()
            .map(|w| Watch {
                kind: w.kind.into(),
                args: w
                    .args
                    .into_iter()
                    .map(|(n, v)| Arg { name: n.into(), value: value(v) })
                    .collect::<Vec<_>>()
                    .into(),
            })
            .collect::<Vec<_>>();
        Service {
            id: spec.id.into(),
            kind: spec.kind.into(),
            party: spec.party.into(),
            about: spec.about.into(),
            rank: spec.rank.map(|(value, s)| Rank { value, source: source(s) }),
            endpoints: endpoints.into(),
            facts: facts.into(),
            watches: watches.into(),
        }
    }
}

#[cfg(feature = "overlay")]
pub use overlay::OverlayError;
