//! The placeholder credentials of a policy (T-100). The job holds a
//! placeholder; the broker swaps it for the real value of a secret on the
//! way to the hosts of the credential, inside its TLS end, and puts the
//! placeholder back in each answer. The policy names the secret; the host
//! gives its value at the load ([`Secrets`]), so that a policy, an offer
//! and an audit line hold no secret value.
//!
//! ```json
//! { "placeholder": "PODCRED_GITHUB_TOKEN_0001", "secret": "github",
//!   "hosts": ["api.github.com", "*.githubusercontent.com"], "in": ["header", "basic"] }
//! ```

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Deserializer, Serialize};
use zeroize::Zeroizing;

use crate::policy::HostRule;
use crate::target::Host;

/// The shortest placeholder: a word of the job's text must not take it by
/// chance.
pub const MIN_PLACEHOLDER: usize = 16;

/// The shortest secret value: a short value would scrub common text.
pub const MIN_SECRET: usize = 8;

/// The longest secret value.
pub const MAX_SECRET: usize = 4096;

/// Where the broker swaps a placeholder in a request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum Place {
    /// The value of each header: `Authorization: Bearer PLACEHOLDER`.
    Header,
    /// The user or the password of `Authorization: Basic`.
    Basic,
    /// The query of the request-target, percent-encoded.
    Query,
    /// The body of the request, within the bounds of the TLS end.
    Body,
}

impl Place {
    pub fn word(self) -> &'static str {
        match self {
            Place::Header => "header",
            Place::Basic => "basic",
            Place::Query => "query",
            Place::Body => "body",
        }
    }
}

/// The places with no `in`: a query and a body can publish a value (a
/// search, a comment), so they need a choice.
pub const DEFAULT_PLACES: [Place; 2] = [Place::Header, Place::Basic];

/// One placeholder credential of a policy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "schema"), derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct Credential {
    /// The text that the job holds: 16 to 256 of `A-Z a-z 0-9 _ . -`.
    #[cfg_attr(any(test, feature = "schema"), schemars(regex(pattern = r"^[A-Za-z0-9_.-]{16,256}$")))]
    pub placeholder: String,
    /// The name of the secret whose value the host gives at the load: 1 to
    /// 128 of `A-Z a-z 0-9 . _ : -`.
    #[cfg_attr(any(test, feature = "schema"), schemars(regex(pattern = r"^[A-Za-z0-9._:-]{1,128}$")))]
    pub secret: String,
    /// The hosts that get the real value: a name, `*.NAME` or an address,
    /// as the host of an allow rule. Each other host gets the placeholder.
    #[cfg_attr(any(test, feature = "schema"), schemars(length(min = 1), inner(length(min = 1, max = 255))))]
    pub hosts: Vec<String>,
    /// Where the swap happens; none: `header` and `basic`.
    #[serde(default, rename = "in", skip_serializing_if = "Vec::is_empty")]
    pub places: Vec<Place>,
}

/// A credential of a checked policy: no secret value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checked {
    pub placeholder: String,
    pub secret: String,
    pub(crate) hosts: Vec<HostRule>,
    pub places: Vec<Place>,
}

impl Checked {
    /// Whether `host` gets the real value.
    pub fn holds(&self, host: &Host) -> bool {
        self.hosts.iter().any(|rule| rule.holds(host))
    }
}

/// The checked form of `credentials`, or why it is not usable.
pub(crate) fn compile(credentials: &[Credential]) -> Result<Vec<Checked>, String> {
    let mut checked: Vec<Checked> = Vec::with_capacity(credentials.len());
    for (n, c) in credentials.iter().enumerate() {
        let fault = |why: String| format!("credential {n}: {why}");
        let fine = |b: u8| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-');
        if !(MIN_PLACEHOLDER..=256).contains(&c.placeholder.len()) || !c.placeholder.bytes().all(fine) {
            return Err(fault(format!("a placeholder has {MIN_PLACEHOLDER} to 256 of A-Z a-z 0-9 _ . -")));
        }
        crate::jobs::check_job_id(&c.secret)
            .map_err(|_| fault("a secret name has 1 to 128 of A-Z a-z 0-9 . _ : -".into()))?;
        if c.hosts.is_empty() {
            return Err(fault("a credential names one host or more".into()));
        }
        let hosts = c.hosts.iter().map(|h| HostRule::parse(h)).collect::<Result<Vec<_>, _>>().map_err(fault)?;
        if let Some(other) =
            checked.iter().find(|o| o.placeholder.contains(&c.placeholder) || c.placeholder.contains(&o.placeholder))
        {
            return Err(fault(format!("its placeholder and the one of the secret {} hold each other", other.secret)));
        }
        let mut places = if c.places.is_empty() { DEFAULT_PLACES.to_vec() } else { c.places.clone() };
        places.sort_unstable();
        places.dedup();
        checked.push(Checked { placeholder: c.placeholder.clone(), secret: c.secret.clone(), hosts, places });
    }
    Ok(checked)
}

/// The values of the secrets of a job, by name, which the host gives at the
/// load. Each value is wiped when it is dropped, and no format shows one.
#[derive(Clone, Default)]
pub struct Secrets(BTreeMap<String, Zeroizing<String>>);

impl Secrets {
    pub fn new() -> Secrets {
        Secrets::default()
    }

    /// The secret `name` with `value`, in place of one of the same name.
    pub fn with(mut self, name: impl Into<String>, value: impl Into<String>) -> Secrets {
        self.0.insert(name.into(), Zeroizing::new(value.into()));
        self
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub(crate) fn get(&self, name: &str) -> Option<&str> {
        self.0.get(name).map(|v| v.as_str())
    }

    pub(crate) fn names(&self) -> impl Iterator<Item = &str> {
        self.0.keys().map(String::as_str)
    }
}

impl fmt::Debug for Secrets {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self.0.keys()).finish()
    }
}

/// One value of [`Secrets`], wiped from its read: a map that fails to read
/// after its first values drops them.
struct SecretValue(Zeroizing<String>);

impl<'de> Deserialize<'de> for SecretValue {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<SecretValue, D::Error> {
        String::deserialize(d).map(|v| SecretValue(Zeroizing::new(v)))
    }
}

impl<'de> Deserialize<'de> for Secrets {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Secrets, D::Error> {
        let map = BTreeMap::<String, SecretValue>::deserialize(d)?;
        Ok(Secrets(map.into_iter().map(|(k, SecretValue(v))| (k, v)).collect()))
    }
}

/// Why the value of a secret is not usable: 8 to 4096 printable ASCII
/// characters with no space, and no placeholder inside it or around it.
pub(crate) fn check_value(value: &str, placeholders: &[&str]) -> Result<(), String> {
    if !(MIN_SECRET..=MAX_SECRET).contains(&value.len()) || !value.bytes().all(|b| b.is_ascii_graphic()) {
        return Err(format!(
            "a secret value has {MIN_SECRET} to {MAX_SECRET} printable ASCII characters with no space"
        ));
    }
    if placeholders.iter().any(|p| p.contains(value) || value.contains(p)) {
        return Err("a secret value and a placeholder hold each other".into());
    }
    Ok(())
}
