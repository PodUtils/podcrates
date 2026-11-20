//! The catalogue of the methods: one table with the wire name, the route,
//! the tier and the summary of each method. The dispatch, the discovery
//! (`methods`) and the JSON Schema are made from it, and the tests hold the
//! served methods to it in both directions.

use std::fmt;

/// What a caller may do. A higher tier holds each lower one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Tier {
    /// Read the state: no change.
    Read,
    /// Change the state of the program now: drain it, stop it.
    Operate,
    /// The master token: a change whose effect outlives the token (a new
    /// principal, a kept forward target, a configuration read again).
    Master,
}

impl Tier {
    pub const ALL: [Tier; 3] = [Tier::Read, Tier::Operate, Tier::Master];

    /// The name on the wire.
    pub fn name(self) -> &'static str {
        match self {
            Tier::Read => "read",
            Tier::Operate => "operate",
            Tier::Master => "master",
        }
    }

    pub fn from_name(name: &str) -> Option<Tier> {
        Tier::ALL.into_iter().find(|t| t.name() == name)
    }
}

impl fmt::Display for Tier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// Where a method is answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Route {
    /// By the server of this crate, also while the host is busy.
    Local,
    /// By the host, through [`crate::Host`].
    Delegated,
}

impl Route {
    pub fn name(self) -> &'static str {
        match self {
            Route::Local => "local",
            Route::Delegated => "delegated",
        }
    }
}

/// The methods of each program.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Method {
    Version,
    Methods,
    Auth,
    Status,
    Metrics,
    Reload,
    Drain,
    Stop,
}

/// One row of the catalogue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Spec {
    pub method: Method,
    /// The name on the wire.
    pub name: &'static str,
    pub route: Route,
    pub tier: Tier,
    /// The effect outlives the token of the caller: such a method takes
    /// [`Tier::Master`].
    pub outlives: bool,
    /// One sentence for `methods` and for the schema.
    pub summary: &'static str,
}

impl Method {
    pub const ALL: [Method; 8] = [
        Method::Version,
        Method::Methods,
        Method::Auth,
        Method::Status,
        Method::Metrics,
        Method::Reload,
        Method::Drain,
        Method::Stop,
    ];

    /// The row of the method.
    pub const fn spec(self) -> Spec {
        const fn row(method: Method, name: &'static str, route: Route, tier: Tier, summary: &'static str) -> Spec {
            Spec { method, name, route, tier, outlives: false, summary }
        }
        use Route::{Delegated, Local};
        use Tier::{Master, Operate, Read};
        match self {
            Method::Version => {
                row(self, "version", Local, Read, "The name and the version of the program, and the protocol.")
            }
            Method::Methods => row(self, "methods", Local, Read, "The methods that this caller can call."),
            Method::Auth => {
                row(self, "auth", Local, Read, "Give the master token: this connection then has the master tier.")
            }
            Method::Status => row(self, "status", Delegated, Read, "The state of the program now."),
            Method::Metrics => row(self, "metrics", Delegated, Read, "The counters and the gauges of the program."),
            Method::Reload => Spec {
                outlives: true,
                ..row(self, "reload", Delegated, Master, "Read the configuration again and use it.")
            },
            Method::Drain => row(
                self,
                "drain",
                Delegated,
                Operate,
                "Take no new work and let the work in flight end; with exit, the program then exits.",
            ),
            Method::Stop => {
                row(self, "stop", Delegated, Operate, "Stop the program now: it ends the work in flight by its rule.")
            }
        }
    }

    /// The name on the wire.
    pub const fn name(self) -> &'static str {
        self.spec().name
    }

    pub fn from_name(name: &str) -> Option<Method> {
        Method::ALL.into_iter().find(|m| m.name() == name)
    }
}

/// A method of the host beyond the catalogue of this crate, answered by
/// [`crate::Host::extra`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Extra {
    /// The name on the wire: a lower-case letter, then at most 62 lower-case
    /// letters, digits and `-`.
    pub name: String,
    pub tier: Tier,
    /// The effect outlives the token of the caller (see [`Spec::outlives`]).
    pub outlives: bool,
    pub summary: String,
}

impl Extra {
    pub fn new(name: &str, tier: Tier, summary: &str) -> Extra {
        Extra { name: name.into(), tier, outlives: false, summary: summary.into() }
    }
}

/// Why a catalogue cannot be made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CatalogueFault {
    /// The name is not of the form of a method name.
    Name(String),
    /// Two methods have the name.
    Twice(String),
    /// An effect that outlives its token, below the master tier.
    Outlives(String),
}

impl fmt::Display for CatalogueFault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CatalogueFault::Name(n) => write!(f, "the method name {n:?} is not a lower-case word with `-`"),
            CatalogueFault::Twice(n) => write!(f, "two methods have the name {n:?}"),
            CatalogueFault::Outlives(n) => {
                write!(f, "the effect of the method {n:?} outlives the token: it must have the master tier")
            }
        }
    }
}

impl std::error::Error for CatalogueFault {}

/// One method of a catalogue: a row of this crate, or an extra of the host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Entry<'a> {
    Core(Spec),
    Extra(&'a Extra),
}

impl Entry<'_> {
    pub fn name(&self) -> &str {
        match self {
            Entry::Core(s) => s.name,
            Entry::Extra(e) => &e.name,
        }
    }

    pub fn tier(&self) -> Tier {
        match self {
            Entry::Core(s) => s.tier,
            Entry::Extra(e) => e.tier,
        }
    }

    pub fn route(&self) -> Route {
        match self {
            Entry::Core(s) => s.route,
            Entry::Extra(_) => Route::Delegated,
        }
    }

    pub fn summary(&self) -> &str {
        match self {
            Entry::Core(s) => s.summary,
            Entry::Extra(e) => &e.summary,
        }
    }

    pub fn outlives(&self) -> bool {
        match self {
            Entry::Core(s) => s.outlives,
            Entry::Extra(e) => e.outlives,
        }
    }
}

/// The methods of one server: the rows of this crate and the extras of its
/// host.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Catalogue {
    extras: Vec<Extra>,
}

/// True for a method name of the wire.
pub fn is_method_name(name: &str) -> bool {
    let b = name.as_bytes();
    (1..=63).contains(&b.len())
        && b[0].is_ascii_lowercase()
        && b.iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-')
}

impl Catalogue {
    /// The rows of this crate alone.
    pub fn core() -> Catalogue {
        Catalogue::default()
    }

    /// The rows of this crate and `extras`. Each name is unique, and a method
    /// whose effect outlives its token has the master tier.
    pub fn new(extras: Vec<Extra>) -> Result<Catalogue, CatalogueFault> {
        for (i, e) in extras.iter().enumerate() {
            if !is_method_name(&e.name) {
                return Err(CatalogueFault::Name(e.name.clone()));
            }
            if Method::from_name(&e.name).is_some() || extras[..i].iter().any(|o| o.name == e.name) {
                return Err(CatalogueFault::Twice(e.name.clone()));
            }
            if e.outlives && e.tier != Tier::Master {
                return Err(CatalogueFault::Outlives(e.name.clone()));
            }
        }
        Ok(Catalogue { extras })
    }

    /// Each method, the rows of this crate first.
    pub fn entries(&self) -> impl Iterator<Item = Entry<'_>> {
        Method::ALL.into_iter().map(|m| Entry::Core(m.spec())).chain(self.extras.iter().map(Entry::Extra))
    }

    /// The method `name`, at any tier.
    pub fn find(&self, name: &str) -> Option<Entry<'_>> {
        self.entries().find(|e| e.name() == name)
    }

    /// The method `name` when a caller of `tier` may call it. A method above
    /// the tier is `None`, as a name that is not there.
    pub fn find_for(&self, name: &str, tier: Tier) -> Option<Entry<'_>> {
        self.find(name).filter(|e| e.tier() <= tier)
    }
}
