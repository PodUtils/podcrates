//! The field classifier: the SNI, the ALPN, the JA3 and JA4 hashes and the
//! TLS record sizes, against a rule set that a JSON file holds.
//!
//! The score is the sum of the weights of the rules that match, capped at
//! one. A higher score means that the fields look more like the road that
//! the rules name. A rule set with no match gives 0.

use crate::capture::{Capture, tls_record_sizes};
use crate::error::{Error, Result};
use crate::tls::ClientHello;
use std::path::Path;

/// One rule of the field matcher.
#[derive(Clone, Debug, PartialEq)]
pub struct Rule {
    pub id: String,
    pub weight: f64,
    pub kind: RuleKind,
}

/// What a rule tests.
#[derive(Clone, Debug, PartialEq)]
pub enum RuleKind {
    SniSuffix(String),
    SniExact(String),
    Alpn(String),
    Ja3(String),
    Ja4(String),
    RecordMedianBelow(usize),
    RecordMedianAbove(usize),
}

/// The field classifier and its rules.
#[derive(Clone, Debug, PartialEq)]
pub struct FieldMatcher {
    rules: Vec<Rule>,
}

/// One rule that matched, for the reason of a score.
#[derive(Clone, Debug, PartialEq)]
pub struct FieldMatch {
    pub id: String,
    pub weight: f64,
    pub detail: String,
}

impl FieldMatcher {
    pub fn new(rules: Vec<Rule>) -> Result<FieldMatcher> {
        let mut seen = std::collections::HashSet::new();
        for rule in &rules {
            if rule.id.trim().is_empty() {
                return Err(Error::new("a field rule has no id"));
            }
            if !seen.insert(rule.id.clone()) {
                return Err(Error::new(format!("the field rule {} is twice", rule.id)));
            }
            if !(0.0..=1.0).contains(&rule.weight) {
                return Err(Error::new(format!("the weight of {} is not in 0..1", rule.id)));
            }
        }
        Ok(FieldMatcher { rules })
    }

    pub fn from_json(text: &str) -> Result<FieldMatcher> {
        let value: serde_json::Value = serde_json::from_str(text)?;
        let rules = value
            .get("rules")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| Error::new("the field model has no rules array"))?;
        let mut out = Vec::with_capacity(rules.len());
        for rule in rules {
            let id = rule
                .get("id")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| Error::new("a field rule has no id"))?;
            let weight = rule
                .get("weight")
                .and_then(serde_json::Value::as_f64)
                .ok_or_else(|| Error::new(format!("the field rule {id} has no weight")))?;
            let kind = rule
                .get("kind")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| Error::new(format!("the field rule {id} has no kind")))?;
            let value = rule.get("value").ok_or_else(|| Error::new(format!("the field rule {id} has no value")))?;
            let text_value = || {
                value.as_str().map(str::to_string).ok_or_else(|| Error::new(format!("the value of {id} is not text")))
            };
            let number = || {
                value
                    .as_u64()
                    .map(|n| n as usize)
                    .ok_or_else(|| Error::new(format!("the value of {id} is not a number")))
            };
            let kind = match kind {
                "sni_suffix" => RuleKind::SniSuffix(text_value()?),
                "sni_exact" => RuleKind::SniExact(text_value()?),
                "alpn" => RuleKind::Alpn(text_value()?),
                "ja3" => RuleKind::Ja3(text_value()?.to_ascii_lowercase()),
                "ja4" => RuleKind::Ja4(text_value()?.to_ascii_lowercase()),
                "record_median_below" => RuleKind::RecordMedianBelow(number()?),
                "record_median_above" => RuleKind::RecordMedianAbove(number()?),
                other => return Err(Error::new(format!("the field rule {id} has the unknown kind {other}"))),
            };
            out.push(Rule { id: id.to_string(), weight, kind });
        }
        FieldMatcher::new(out)
    }

    pub fn load(path: &Path) -> Result<FieldMatcher> {
        let text = std::fs::read_to_string(path).map_err(|e| Error::io("read the field model", path, e))?;
        FieldMatcher::from_json(&text)
    }

    pub fn rules(&self) -> &[Rule] {
        &self.rules
    }

    /// The rules that the capture matches, each with what it read.
    pub fn matches(&self, capture: &Capture) -> Vec<FieldMatch> {
        let hello = client_hello(capture);
        let median = median(tls_record_sizes_from_capture(capture));
        let mut out = Vec::new();
        for rule in &self.rules {
            let detail = match (&rule.kind, &hello) {
                (RuleKind::SniSuffix(suffix), Some(h)) => h
                    .sni
                    .as_deref()
                    .filter(|sni| sni == suffix || sni.ends_with(&format!(".{suffix}")))
                    .map(|sni| format!("sni {sni} ends with {suffix}")),
                (RuleKind::SniExact(name), Some(h)) => {
                    h.sni.as_deref().filter(|sni| *sni == name).map(|sni| format!("sni {sni}"))
                }
                (RuleKind::Alpn(protocol), Some(h)) => {
                    h.alpn.iter().find(|p| p.as_slice() == protocol.as_bytes()).map(|_| format!("alpn {protocol}"))
                }
                (RuleKind::Ja3(hash), Some(h)) => {
                    let have = h.ja3();
                    (have == *hash).then(|| format!("ja3 {have}"))
                }
                (RuleKind::Ja4(hash), Some(h)) => {
                    let have = h.ja4();
                    (have == *hash).then(|| format!("ja4 {have}"))
                }
                (RuleKind::RecordMedianBelow(limit), _) => {
                    median.filter(|m| *m < *limit).map(|m| format!("record median {m} is below {limit}"))
                }
                (RuleKind::RecordMedianAbove(limit), _) => {
                    median.filter(|m| *m > *limit).map(|m| format!("record median {m} is above {limit}"))
                }
                _ => None,
            };
            if let Some(detail) = detail {
                out.push(FieldMatch { id: rule.id.clone(), weight: rule.weight, detail });
            }
        }
        out
    }

    /// A score in 0..1, and the names of the rules that matched.
    pub fn score(&self, capture: &Capture) -> (f64, String) {
        if client_hello(capture).is_none() {
            return (0.0, "no TLS ClientHello".to_string());
        }
        let matches = self.matches(capture);
        if matches.is_empty() {
            return (0.0, "no field rule matched".to_string());
        }
        let score = matches.iter().map(|m| m.weight).sum::<f64>().clamp(0.0, 1.0);
        let reason = matches.iter().map(|m| format!("{}: {}", m.id, m.detail)).collect::<Vec<_>>().join("; ");
        (score, reason)
    }
}

fn client_hello(capture: &Capture) -> Option<ClientHello> {
    if let Some(hello) = capture
        .packets
        .iter()
        .find(|p| p.direction == crate::capture::Direction::ClientToServer && p.bytes.first() == Some(&0x16))
        .and_then(|p| ClientHello::parse_record(&p.bytes).ok())
    {
        return Some(hello);
    }
    // The ClientHello can run over more than one packet. Join the client's
    // bytes and read the first record of the result.
    let joined: Vec<u8> = capture
        .packets
        .iter()
        .filter(|p| p.direction == crate::capture::Direction::ClientToServer)
        .flat_map(|p| p.bytes.iter().copied())
        .collect();
    ClientHello::parse_record(&joined).ok()
}

fn tls_record_sizes_from_capture(capture: &Capture) -> Vec<usize> {
    capture.packets.iter().flat_map(|p| tls_record_sizes(&p.bytes)).collect()
}

fn median(mut values: Vec<usize>) -> Option<usize> {
    if values.is_empty() {
        return None;
    }
    values.sort_unstable();
    Some(values[values.len() / 2])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn capture(name: &str) -> Capture {
        Capture::parse(include_str!(concat!("../fixtures/captures/", "tunnel.cap")))
            .unwrap_or_else(|e| panic!("{name}: {e}"))
    }

    fn rules() -> FieldMatcher {
        FieldMatcher::from_json(include_str!("../fixtures/rules.json")).unwrap()
    }

    #[test]
    fn the_tunnel_fixture_matches_the_rule_set() {
        let (score, reason) = rules().score(&capture("tunnel"));
        assert!((0.0..=1.0).contains(&score), "{score}");
        assert!(score > 0.9, "{score}: {reason}");
        assert!(reason.contains("sni"), "{reason}");
        assert!(reason.contains("ja3"), "{reason}");
        assert!(reason.contains("ja4"), "{reason}");
    }

    #[test]
    fn the_benign_fixture_keeps_a_bounded_score() {
        let benign = Capture::parse(include_str!("../fixtures/captures/benign.cap")).unwrap();
        let (score, reason) = rules().score(&benign);
        assert_eq!(score, 0.0, "{reason}");
    }

    #[test]
    fn a_padded_tunnel_has_a_lower_score_than_the_unpadded_one() {
        use crate::middlebox::{Fault, Middlebox};
        let plain = Capture::parse(include_str!("../fixtures/captures/tunnel.cap")).unwrap();
        let (plain_score, _) = rules().score(&plain);
        let mut box_ = Middlebox::new(vec![Fault::Pad { to: 1200 }]);
        let padded = box_.apply(&plain);
        let (padded_score, reason) = rules().score(&padded);
        assert!(padded_score < plain_score, "{padded_score} is not below {plain_score}: {reason}");
    }

    #[test]
    fn a_rule_set_that_is_not_valid_is_refused() {
        assert!(FieldMatcher::from_json("{}").is_err());
        assert!(FieldMatcher::from_json(r#"{"rules":[{"id":"x","weight":2,"kind":"alpn","value":"h2"}]}"#).is_err());
        assert!(FieldMatcher::from_json(r#"{"rules":[{"id":"x","weight":0.2,"kind":"nope","value":"h2"}]}"#).is_err());
        assert!(
            FieldMatcher::from_json(
                r#"{"rules":[{"id":"x","weight":0.2,"kind":"alpn","value":"h2"},{"id":"x","weight":0.2,"kind":"alpn","value":"h2"}]}"#
            )
            .is_err()
        );
    }

    #[test]
    fn an_empty_capture_has_no_field_match() {
        let empty = Capture::new();
        let (score, reason) = rules().score(&empty);
        assert_eq!(score, 0.0);
        assert_eq!(reason, "no TLS ClientHello");
        assert_eq!(rules().matches(&empty).len(), 0);
    }
}
