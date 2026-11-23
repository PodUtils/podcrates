//! The declared shape, the observed shape and their comparison. A
//! divergence is a warning in words, never an error.

use crate::Profile;

/// The padded bytes over the body bytes of a transfer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PaddedFraction {
    pub padded: u64,
    pub total: u64,
}

impl PaddedFraction {
    pub fn of(padded: u64, total: u64) -> PaddedFraction {
        PaddedFraction { padded, total }
    }

    /// The padded bytes over the total bytes.
    pub fn fraction(&self) -> f64 {
        if self.total == 0 { 0.0 } else { self.padded as f64 / self.total as f64 }
    }

    /// The same, in percent.
    pub fn percent(&self) -> f64 {
        self.fraction() * 100.0
    }
}

/// The distribution of the body sizes, and the padded fraction where it is
/// known.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Shape {
    /// Each body size and its fraction of the records, in increasing order.
    pub buckets: Vec<(usize, f64)>,
    /// The padded bytes over the total bytes, where a transfer measured it.
    pub padded: Option<f64>,
}

impl Shape {
    /// The shape of `samples`, with the padded fraction of `padded`.
    pub fn of(samples: &[usize], padded: PaddedFraction) -> Shape {
        let mut buckets: Vec<(usize, f64)> = Vec::new();
        for size in samples {
            match buckets.iter_mut().find(|(bucket, _)| bucket == size) {
                Some((_, count)) => *count += 1.0,
                None => buckets.push((*size, 1.0)),
            }
        }
        let total = samples.len().max(1) as f64;
        for (_, count) in &mut buckets {
            *count /= total;
        }
        buckets.sort_by_key(|(size, _)| *size);
        Shape { buckets, padded: Some(padded.fraction()) }
    }

    /// The declared shape of `profile`: its weights as fractions. The
    /// padded fraction is not known before the records arrive.
    pub fn of_profile(profile: &Profile) -> Shape {
        let total: f64 = profile.buckets.iter().map(|b| f64::from(b.weight)).sum();
        let mut buckets: Vec<(usize, f64)> =
            profile.buckets.iter().map(|b| (b.size, f64::from(b.weight) / total)).collect();
        buckets.sort_by_key(|(size, _)| *size);
        Shape { buckets, padded: None }
    }

    /// The fraction of the records of `size`.
    pub fn bucket(&self, size: usize) -> f64 {
        self.buckets.iter().find(|(bucket, _)| *bucket == size).map_or(0.0, |(_, fraction)| *fraction)
    }
}

/// The state of a shape, as the three states of `pod-lab`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// Each bucket is inside the tolerance.
    Holds,
    /// A bucket or the padded fraction is outside the tolerance.
    Fails,
    /// No record was observed.
    Unseen,
}

impl State {
    pub fn word(self) -> &'static str {
        match self {
            State::Holds => "holds",
            State::Fails => "fails",
            State::Unseen => "unseen",
        }
    }
}

/// The declared shape beside the observed one.
#[derive(Debug, Clone, PartialEq)]
pub struct ShapeReport {
    pub declared: Shape,
    pub observed: Option<Shape>,
    pub state: State,
    /// One line for each divergence, in words.
    pub warnings: Vec<String>,
}

impl ShapeReport {
    /// The tolerance of one bucket, as a fraction of the records.
    pub const TOLERANCE: f64 = 0.05;

    /// Compare the declared shape of a profile with an observed one. No
    /// sample is [`State::Unseen`]; a bucket or a padded fraction outside
    /// [`ShapeReport::TOLERANCE`] is [`State::Fails`] with a warning.
    pub fn compare(declared: Shape, observed: Option<Shape>) -> ShapeReport {
        let Some(observed) = observed else {
            return ShapeReport { declared, observed: None, state: State::Unseen, warnings: Vec::new() };
        };
        let mut warnings = Vec::new();
        let mut sizes: Vec<usize> = declared.buckets.iter().map(|(size, _)| *size).collect();
        for (size, _) in &observed.buckets {
            if !sizes.contains(size) {
                sizes.push(*size);
            }
        }
        sizes.sort_unstable();
        for size in sizes {
            let want = declared.bucket(size);
            let got = observed.bucket(size);
            if (want - got).abs() > ShapeReport::TOLERANCE {
                warnings.push(format!("bucket {size}: declared {want:.3}, observed {got:.3}"));
            }
        }
        if let (Some(want), Some(got)) = (declared.padded, observed.padded)
            && (want - got).abs() > ShapeReport::TOLERANCE
        {
            warnings.push(format!("padded fraction: declared {want:.3}, observed {got:.3}"));
        }
        let state = if warnings.is_empty() { State::Holds } else { State::Fails };
        ShapeReport { declared, observed: Some(observed), state, warnings }
    }
}
