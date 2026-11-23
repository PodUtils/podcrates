//! The limits page names, for each limit, the condition that triggers it and
//! the check that detects it. A split behaviour with no limit fails.

use pod_sidecar::Behaviour;
use pod_sidecar::limits::{LIMITS, limit_of, split_limits};

#[test]
fn every_limit_names_its_condition_and_its_detection() {
    assert!(!LIMITS.is_empty());
    for limit in LIMITS {
        assert!(!limit.behaviour.is_empty());
        assert!(!limit.condition.is_empty(), "{} has no condition", limit.behaviour);
        assert!(!limit.detection.is_empty(), "{} has no detection", limit.behaviour);
    }
}

#[test]
fn every_split_behaviour_has_a_limit_with_a_detection() {
    assert_eq!(split_limits().len(), Behaviour::ALL.len());
    for behaviour in Behaviour::ALL {
        let limit = limit_of(behaviour.name())
            .unwrap_or_else(|| panic!("{} has no limit in the limits page", behaviour.name()));
        assert!(limit.detection.contains("Simulator::split"), "{}: {}", behaviour.name(), limit.detection);
    }
}

#[test]
fn the_other_limits_are_named_too() {
    for name in ["fake-tcp", "raw-socket-probe", "silent-probe", "bond-one-flow", "bond-move"] {
        let limit = limit_of(name).unwrap_or_else(|| panic!("{name} has no limit"));
        assert!(!limit.detection.is_empty());
    }
}

#[test]
fn a_limit_for_a_named_behaviour_is_unique() {
    let mut seen = std::collections::BTreeSet::new();
    for limit in LIMITS {
        assert!(seen.insert(limit.behaviour), "{} is listed twice", limit.behaviour);
    }
}
