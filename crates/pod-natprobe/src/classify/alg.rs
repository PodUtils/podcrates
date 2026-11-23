//! NAT ALG detection across all validated STUN responses (never stops at first match).

use crate::report::AlgDetection;
use crate::wire::Validated;

/// Combine ALG observations from all validated STUN responses collected during a run.
/// `Suspected` dominates `NoAlgObserved`, which dominates `Unverifiable` and `Undetectable`.
pub fn alg_from_responses(responses: &[&Validated]) -> Option<AlgDetection> {
    let mut saw_clean = false;
    let mut saw_xor_only = false;
    let mut saw_plain_only = false;
    for r in responses {
        match (r.xor_mapped(), r.plain_mapped()) {
            (Some(xor), Some(plain)) if xor != plain => {
                return Some(AlgDetection::Suspected { plain, xor });
            }
            (Some(_), Some(_)) => saw_clean = true,
            (Some(_), None) => saw_xor_only = true,
            (None, Some(_)) => saw_plain_only = true,
            (None, None) => {}
        }
    }
    if saw_clean {
        Some(AlgDetection::NoAlgObserved)
    } else if saw_xor_only {
        Some(AlgDetection::Unverifiable)
    } else if saw_plain_only {
        Some(AlgDetection::Undetectable)
    } else {
        None
    }
}
