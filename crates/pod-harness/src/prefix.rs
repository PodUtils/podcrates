//! The first-N-bytes classifier: a byte n-gram table that a JSON file
//! holds, matched against the first bytes of the client's first packet.
//!
//! The score is the share of the table's weight that the prefix holds, from
//! 0 to 1. It is cheap, and it is the last classifier of a run.

use crate::capture::{Capture, Direction, decode_hex};
use crate::error::{Error, Result};
use std::path::Path;

/// One n-gram of the table.
#[derive(Clone, Debug, PartialEq)]
pub struct Gram {
    pub label: String,
    pub weight: f64,
    /// The bytes of the gram, in the order that they appear.
    pub bytes: Vec<u8>,
}

/// The prefix classifier and its table.
#[derive(Clone, Debug, PartialEq)]
pub struct Prefix {
    n: usize,
    prefix_len: usize,
    grams: Vec<Gram>,
}

impl Prefix {
    pub fn new(n: usize, prefix_len: usize, grams: Vec<Gram>) -> Result<Prefix> {
        if n == 0 {
            return Err(Error::new("the prefix model has n = 0"));
        }
        if prefix_len < n {
            return Err(Error::new("the prefix length is below n"));
        }
        if grams.is_empty() {
            return Err(Error::new("the prefix model has no gram"));
        }
        for gram in &grams {
            if gram.bytes.len() != n {
                return Err(Error::new(format!("the gram {} has {} bytes, not {n}", gram.label, gram.bytes.len())));
            }
            if !(0.0..=1.0).contains(&gram.weight) {
                return Err(Error::new(format!("the weight of {} is not in 0..1", gram.label)));
            }
        }
        Ok(Prefix { n, prefix_len, grams })
    }

    pub fn from_json(text: &str) -> Result<Prefix> {
        let value: serde_json::Value = serde_json::from_str(text)?;
        let n =
            value.get("n").and_then(serde_json::Value::as_u64).ok_or_else(|| Error::new("the prefix model has no n"))?
                as usize;
        let prefix_len = value
            .get("prefix_len")
            .and_then(serde_json::Value::as_u64)
            .ok_or_else(|| Error::new("the prefix model has no prefix_len"))? as usize;
        let grams = value
            .get("grams")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| Error::new("the prefix model has no grams array"))?;
        let mut out = Vec::with_capacity(grams.len());
        for gram in grams {
            let label = gram
                .get("label")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| Error::new("a gram has no label"))?;
            let weight = gram
                .get("weight")
                .and_then(serde_json::Value::as_f64)
                .ok_or_else(|| Error::new(format!("the gram {label} has no weight")))?;
            let text = gram
                .get("gram")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| Error::new(format!("the gram {label} has no bytes")))?;
            let bytes = decode_hex(text)?;
            out.push(Gram { label: label.to_string(), weight, bytes });
        }
        Prefix::new(n, prefix_len, out)
    }

    pub fn load(path: &Path) -> Result<Prefix> {
        let text = std::fs::read_to_string(path).map_err(|e| Error::io("read the prefix model", path, e))?;
        Prefix::from_json(&text)
    }

    pub fn n(&self) -> usize {
        self.n
    }

    pub fn prefix_len(&self) -> usize {
        self.prefix_len
    }

    pub fn grams(&self) -> &[Gram] {
        &self.grams
    }

    /// The score in 0..1, and the labels of the grams that matched.
    pub fn score(&self, capture: &Capture) -> (f64, String) {
        let Some(packet) = capture.packets.iter().find(|p| p.direction == Direction::ClientToServer) else {
            return (0.0, "no packet from the client".to_string());
        };
        if packet.bytes.len() < self.n {
            return (0.0, format!("the client packet has {} bytes, below n = {}", packet.bytes.len(), self.n));
        }
        let prefix = &packet.bytes[..packet.bytes.len().min(self.prefix_len)];
        let total: f64 = self.grams.iter().map(|g| g.weight).sum();
        if total <= 0.0 {
            return (0.0, "the table has no weight".to_string());
        }
        let mut matched_weight = 0.0;
        let mut labels: Vec<&str> = Vec::new();
        let mut count = 0;
        for gram in &self.grams {
            if prefix.windows(self.n).any(|window| window == gram.bytes) {
                matched_weight += gram.weight;
                count += 1;
                if !labels.contains(&gram.label.as_str()) {
                    labels.push(&gram.label);
                }
            }
        }
        if count == 0 {
            return (0.0, "no gram matched".to_string());
        }
        let score = (matched_weight / total).clamp(0.0, 1.0);
        (score, format!("{} of {} grams: {}", count, self.grams.len(), labels.join(", ")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn capture(name: &str) -> Capture {
        let text = match name {
            "tunnel" => include_str!("../fixtures/captures/tunnel.cap"),
            "benign" => include_str!("../fixtures/captures/benign.cap"),
            other => panic!("no fixture {other}"),
        };
        Capture::parse(text).unwrap()
    }

    fn model() -> Prefix {
        Prefix::from_json(include_str!("../fixtures/prefix.json")).unwrap()
    }

    #[test]
    fn the_tunnel_prefix_matches_the_table() {
        let (score, reason) = model().score(&capture("tunnel"));
        assert!(score > 0.8, "{score}: {reason}");
        assert!(reason.contains("sni-tunnel"), "{reason}");
    }

    #[test]
    fn the_benign_prefix_has_a_lower_score() {
        let (tunnel, _) = model().score(&capture("tunnel"));
        let (benign, reason) = model().score(&capture("benign"));
        assert!(benign < tunnel, "{benign} is not below {tunnel}: {reason}");
        assert!((0.0..=1.0).contains(&benign));
    }

    #[test]
    fn a_short_packet_and_a_bad_model_are_named() {
        let mut short = Capture::new();
        short.push(crate::capture::Packet::new(Direction::ClientToServer, 0, [0x16]));
        let (score, reason) = model().score(&short);
        assert_eq!(score, 0.0);
        assert!(reason.contains("below n"), "{reason}");
        assert!(Prefix::from_json("{}").is_err());
        assert!(Prefix::from_json(r#"{"n":0,"prefix_len":4,"grams":[]}"#).is_err());
        assert!(
            Prefix::from_json(r#"{"n":2,"prefix_len":4,"grams":[{"label":"x","weight":0.5,"gram":"00"}]}"#).is_err()
        );
    }
}
