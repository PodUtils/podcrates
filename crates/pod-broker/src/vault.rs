//! What the TLS end of one job holds (T-100): the CA of the job, the value
//! of each secret of its credentials, and the forms of each value that the
//! scrub of an answer looks for. A job with no credential and no rule with
//! a path has none.

use std::time::Duration;

use base64::Engine as _;
use pod_core::{Class, Failure, Stage};
use zeroize::Zeroizing;

use crate::ca::JobCa;
use crate::credentials::{Checked, Place, Secrets, check_value};
use crate::policy::Compiled;
use crate::replace::{json_escaped, percent};
use crate::target::Host;

type Pairs = Vec<(Vec<u8>, Vec<u8>)>;

/// One credential with its value.
struct Swap {
    checked: Checked,
    value: Zeroizing<String>,
}

/// The CA and the secrets of one job.
pub struct Vault {
    ca: JobCa,
    swaps: Vec<Swap>,
}

impl std::fmt::Debug for Vault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let names: Vec<&str> = self.swaps.iter().map(|s| s.checked.secret.as_str()).collect();
        f.debug_struct("Vault").field("secrets", &names).finish_non_exhaustive()
    }
}

fn usage(words: String) -> Failure {
    Failure::new(Stage::Proxy, Class::Usage, words)
}

impl Vault {
    /// The vault of the job `job`: each secret that a credential of
    /// `policy` names must be in `secrets`, and no other.
    pub fn new(job: &str, ttl: Duration, policy: &Compiled, secrets: &Secrets) -> Result<Vault, Failure> {
        let placeholders: Vec<&str> = policy.credentials().iter().map(|c| c.placeholder.as_str()).collect();
        let mut swaps = Vec::with_capacity(placeholders.len());
        for (n, checked) in policy.credentials().iter().enumerate() {
            let Some(value) = secrets.get(&checked.secret) else {
                return Err(usage(format!(
                    "credential {n} names the secret {}, which the load does not give",
                    checked.secret
                )));
            };
            check_value(value, &placeholders).map_err(|why| usage(format!("the secret {}: {why}", checked.secret)))?;
            swaps.push(Swap { checked: checked.clone(), value: Zeroizing::new(value.to_string()) });
        }
        if let Some(extra) = secrets.names().find(|name| !swaps.iter().any(|s| s.checked.secret == *name)) {
            return Err(usage(format!("the load gives the secret {extra}, which no credential names")));
        }
        let ca = JobCa::new(job, ttl).map_err(|why| Failure::new(Stage::Host, Class::Unavailable, why))?;
        Ok(Vault { ca, swaps })
    }

    pub fn ca(&self) -> &JobCa {
        &self.ca
    }

    /// Whether the job has a secret: each answer of its TLS end is scrubbed.
    pub fn has_secrets(&self) -> bool {
        !self.swaps.is_empty()
    }

    /// Whether a credential gives its value to `host`.
    pub fn names(&self, host: &Host) -> bool {
        self.swaps.iter().any(|s| s.checked.holds(host))
    }

    /// Each placeholder that `host` gets the value of in `place`, with the
    /// value in the form of the place: percent-encoded in a query.
    pub(crate) fn outgoing(&self, host: &Host, place: Place) -> Pairs {
        let held = self.swaps.iter().filter(|s| s.checked.holds(host) && s.checked.places.contains(&place));
        held.map(|s| {
            let value = if place == Place::Query { percent(&s.value) } else { s.value.to_string() };
            (s.checked.placeholder.as_bytes().to_vec(), value.into_bytes())
        })
        .collect()
    }

    /// Each form of each value that an answer can hold, and the form of its
    /// placeholder that takes its place: the value, percent-encoded, in a
    /// string of JSON, and in base64.
    pub(crate) fn scrub(&self) -> Pairs {
        let b64 = |v: &str| base64::engine::general_purpose::STANDARD.encode(v);
        let mut pairs = Vec::new();
        for s in &self.swaps {
            let (value, placeholder) = (s.value.as_str(), s.checked.placeholder.as_str());
            for (from, to) in [
                (value.to_string(), placeholder.to_string()),
                (percent(value), placeholder.to_string()),
                (json_escaped(value), placeholder.to_string()),
                (b64(value), b64(placeholder)),
            ] {
                if !pairs.iter().any(|(f, _): &(Vec<u8>, Vec<u8>)| f == from.as_bytes()) {
                    pairs.push((from.into_bytes(), to.into_bytes()));
                }
            }
        }
        pairs
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policy::Policy;

    #[test]
    fn the_scrub_takes_each_form_of_a_value_back_to_its_placeholder() {
        let policy = Policy::from_json(
            r#"{"schema":1,"mode":"open","credentials":[{"placeholder":"PODCRED_PLACEHOLDER_01","secret":"s","hosts":["a.example"],"in":["query"]}]}"#,
        )
        .unwrap();
        let compiled = policy.compile().unwrap();
        let value = "made-up+value/with\"quote";
        let vault = Vault::new("job1", Duration::from_secs(60), &compiled, &Secrets::new().with("s", value)).unwrap();
        let text = |pairs: &Pairs| -> Vec<(String, String)> {
            pairs.iter().map(|(a, b)| (String::from_utf8_lossy(a).into(), String::from_utf8_lossy(b).into())).collect()
        };
        let ph = "PODCRED_PLACEHOLDER_01".to_string();
        let b64 = |v: &str| base64::engine::general_purpose::STANDARD.encode(v);
        let pct = "made-up%2Bvalue%2Fwith%22quote".to_string();
        let expected = [
            (value.to_string(), ph.clone()),
            (pct.clone(), ph.clone()),
            (r#"made-up+value/with\"quote"#.to_string(), ph.clone()),
            (b64(value), b64(&ph)),
        ];
        assert_eq!(text(&vault.scrub()), expected);
        let host = Host::Name("a.example".into());
        assert_eq!(text(&vault.outgoing(&host, Place::Query)), [(ph, pct)]);
        assert!(vault.outgoing(&host, Place::Header).is_empty());
        assert!(vault.outgoing(&Host::Name("b.example".into()), Place::Query).is_empty());
        assert!(!format!("{vault:?}").contains(value));
    }
}
