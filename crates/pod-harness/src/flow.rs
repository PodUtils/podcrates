//! The flow classifier: packet sizes and timing, against a small
//! nearest-centroid model that a JSON file holds.
//!
//! The features are the mean, the standard deviation and the 10th, 50th and
//! 90th percentile of the packet sizes, then an eight-bucket histogram of
//! the inter-arrival times. The score is the share of the benign and the
//! tunnel centroid that the capture is nearest to: 1 is the tunnel.

use crate::capture::{Capture, tls_record_sizes};
use crate::error::{Error, Result};
use std::path::Path;

/// The number of features of the model.
pub const FEATURES: usize = 13;
/// The top of the size scale: a full Ethernet frame.
const SIZE_SCALE: f64 = 1500.0;
/// The top of each bucket of inter-arrival time, in microseconds. The
/// eighth bucket holds the rest.
const BUCKETS: [f64; 7] = [1_000.0, 5_000.0, 10_000.0, 25_000.0, 50_000.0, 100_000.0, 250_000.0];

/// One class of the model.
#[derive(Clone, Debug, PartialEq)]
pub struct Centroid {
    pub name: String,
    pub vector: [f64; FEATURES],
}

/// The flow classifier and its centroids.
#[derive(Clone, Debug, PartialEq)]
pub struct FlowStats {
    classes: Vec<Centroid>,
}

/// The measured distribution of one capture.
#[derive(Clone, Debug, PartialEq)]
pub struct FlowFeatures {
    pub mean: f64,
    pub variance: f64,
    pub p10: f64,
    pub p50: f64,
    pub p90: f64,
    pub histogram: [f64; 8],
}

impl FlowStats {
    pub fn new(classes: Vec<Centroid>) -> Result<FlowStats> {
        if classes.is_empty() {
            return Err(Error::new("the flow model has no class"));
        }
        let mut seen = std::collections::HashSet::new();
        for class in &classes {
            if class.name.trim().is_empty() {
                return Err(Error::new("a flow class has no name"));
            }
            if !seen.insert(class.name.clone()) {
                return Err(Error::new(format!("the flow class {} is twice", class.name)));
            }
            if class.vector.iter().any(|v| !v.is_finite()) {
                return Err(Error::new(format!("the centroid of {} has a value that is not finite", class.name)));
            }
        }
        Ok(FlowStats { classes })
    }

    pub fn from_json(text: &str) -> Result<FlowStats> {
        let value: serde_json::Value = serde_json::from_str(text)?;
        let classes = value
            .get("classes")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| Error::new("the flow model has no classes array"))?;
        let mut out = Vec::with_capacity(classes.len());
        for class in classes {
            let name = class
                .get("name")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| Error::new("a flow class has no name"))?;
            let values = class
                .get("centroid")
                .and_then(serde_json::Value::as_array)
                .ok_or_else(|| Error::new(format!("the flow class {name} has no centroid")))?;
            if values.len() != FEATURES {
                return Err(Error::new(format!("the centroid of {name} has {} values, not {FEATURES}", values.len())));
            }
            let mut vector = [0.0; FEATURES];
            for (slot, value) in vector.iter_mut().zip(values) {
                *slot = value
                    .as_f64()
                    .ok_or_else(|| Error::new(format!("the centroid of {name} has a value that is not a number")))?;
            }
            out.push(Centroid { name: name.to_string(), vector });
        }
        FlowStats::new(out)
    }

    pub fn load(path: &Path) -> Result<FlowStats> {
        let text = std::fs::read_to_string(path).map_err(|e| Error::io("read the flow model", path, e))?;
        FlowStats::from_json(&text)
    }

    pub fn classes(&self) -> &[Centroid] {
        &self.classes
    }

    /// The score in 0..1, and the nearest class with its distance.
    pub fn score(&self, capture: &Capture) -> (f64, String) {
        let Some(features) = FlowFeatures::measure(capture) else {
            return (0.0, "no packet".to_string());
        };
        let vector = features.vector();
        let distance =
            |class: &Centroid| class.vector.iter().zip(vector).map(|(a, b)| (a - b) * (a - b)).sum::<f64>().sqrt();
        let tunnel = self.classes.iter().find(|c| c.name == "tunnel");
        let benign = self.classes.iter().find(|c| c.name == "benign");
        match (tunnel, benign) {
            (Some(tunnel), Some(benign)) => {
                let dt = distance(tunnel);
                let db = distance(benign);
                let score = if dt + db == 0.0 { 0.5 } else { db / (dt + db) };
                (score.clamp(0.0, 1.0), format!("tunnel distance {dt:.3}, benign distance {db:.3}"))
            }
            (Some(tunnel), None) => {
                let distance = distance(tunnel);
                ((1.0 / (1.0 + distance)).clamp(0.0, 1.0), format!("tunnel distance {distance:.3}"))
            }
            (None, _) => {
                let (class, distance) = self
                    .classes
                    .iter()
                    .map(|c| (c, distance(c)))
                    .min_by(|a, b| a.1.total_cmp(&b.1))
                    .expect("a model has at least one class");
                ((1.0 / (1.0 + distance)).clamp(0.0, 1.0), format!("nearest {} distance {distance:.3}", class.name))
            }
        }
    }
}

impl FlowFeatures {
    /// The distribution of a capture, or None when it holds no packet.
    pub fn measure(capture: &Capture) -> Option<FlowFeatures> {
        if capture.packets.is_empty() {
            return None;
        }
        let mut sizes = Vec::new();
        for packet in &capture.packets {
            let records = tls_record_sizes(&packet.bytes);
            if records.is_empty() {
                sizes.push(packet.bytes.len() as f64);
            } else {
                sizes.extend(records.into_iter().map(|n| n as f64));
            }
        }
        let count = sizes.len() as f64;
        let mean = sizes.iter().sum::<f64>() / count;
        let variance = sizes.iter().map(|s| (s - mean) * (s - mean)).sum::<f64>() / count;
        sizes.sort_by(f64::total_cmp);
        let p10 = quantile(&sizes, 0.10);
        let p50 = quantile(&sizes, 0.50);
        let p90 = quantile(&sizes, 0.90);
        let mut histogram = [0.0; 8];
        let deltas: Vec<f64> = capture
            .packets
            .windows(2)
            .map(|pair| pair[1].timestamp_micros.saturating_sub(pair[0].timestamp_micros) as f64)
            .collect();
        if !deltas.is_empty() {
            for delta in &deltas {
                let slot = BUCKETS.iter().position(|top| delta < top).unwrap_or(7);
                histogram[slot] += 1.0;
            }
            for slot in &mut histogram {
                *slot /= deltas.len() as f64;
            }
        }
        Some(FlowFeatures { mean, variance, p10, p50, p90, histogram })
    }

    /// The feature vector of the model.
    pub fn vector(&self) -> [f64; FEATURES] {
        [
            self.mean / SIZE_SCALE,
            self.variance.sqrt() / SIZE_SCALE,
            self.p10 / SIZE_SCALE,
            self.p50 / SIZE_SCALE,
            self.p90 / SIZE_SCALE,
            self.histogram[0],
            self.histogram[1],
            self.histogram[2],
            self.histogram[3],
            self.histogram[4],
            self.histogram[5],
            self.histogram[6],
            self.histogram[7],
        ]
    }
}

fn quantile(sorted: &[f64], q: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let index = ((sorted.len() - 1) as f64 * q).round() as usize;
    sorted[index]
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

    fn model() -> FlowStats {
        FlowStats::from_json(include_str!("../fixtures/flow.json")).unwrap()
    }

    #[test]
    fn the_tunnel_fixture_is_nearer_the_tunnel_centroid() {
        let (score, reason) = model().score(&capture("tunnel"));
        assert!(score > 0.5, "{score}: {reason}");
        assert!(reason.contains("tunnel"), "{reason}");
    }

    #[test]
    fn the_benign_fixture_is_nearer_the_benign_centroid() {
        let (score, reason) = model().score(&capture("benign"));
        assert!(score < 0.5, "{score}: {reason}");
        assert!((0.0..=1.0).contains(&score));
    }

    #[test]
    fn the_features_are_deterministic() {
        let a = FlowFeatures::measure(&capture("tunnel")).unwrap();
        let b = FlowFeatures::measure(&capture("tunnel")).unwrap();
        assert_eq!(a, b);
        let vector = a.vector();
        assert_eq!(vector.len(), FEATURES);
        assert!(vector.iter().all(|v| v.is_finite()));
        assert!((a.histogram.iter().sum::<f64>() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn an_empty_capture_and_a_bad_model_are_refused() {
        let empty = Capture::new();
        assert_eq!(model().score(&empty), (0.0, "no packet".to_string()));
        assert!(FlowStats::from_json("{}").is_err());
        assert!(FlowStats::from_json(r#"{"classes":[{"name":"x","centroid":[0,0]}]}"#).is_err());
        assert!(FlowStats::from_json(r#"{"classes":[]}"#).is_err());
    }
}
