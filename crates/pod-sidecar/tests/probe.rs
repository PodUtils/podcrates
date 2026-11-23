//! The disagreement probe: a known disagreement is found, a silent path
//! agrees, and a device that cannot run is Unknown with its reason.

use pod_sidecar::probe::{Silent, Unavailable, raw_capability};
use pod_sidecar::{Policy, Probe, Simulator, Verdict};

#[test]
fn a_simulator_with_a_known_disagreement_is_found() {
    let mut simulator = Simulator::new(42).with_policy(Policy::disagreeing());
    let finding = Probe::run(&mut simulator);
    assert_eq!(finding.verdict, Verdict::Disagrees, "{}", finding.reason);
    assert!(!finding.reason.is_empty());
    assert!(finding.checks.iter().any(|check| check.outcome != pod_sidecar::probe::CheckOutcome::Agrees));
}

#[test]
fn a_silent_path_agrees() {
    let mut silent = Silent;
    let finding = Probe::run(&mut silent);
    assert_eq!(finding.verdict, Verdict::Agrees);
    assert!(finding.reason.contains("no path disagreed"));
    assert!(finding.checks.iter().all(|check| check.outcome == pod_sidecar::probe::CheckOutcome::Silent));
}

#[test]
fn a_correct_simulator_agrees() {
    let mut simulator = Simulator::new(42);
    let finding = Probe::run(&mut simulator);
    assert_eq!(finding.verdict, Verdict::Agrees, "{}", finding.reason);
}

#[test]
fn an_unavailable_device_is_unknown_with_a_named_reason() {
    let mut device = Unavailable::new("raw socket unavailable: EPERM");
    let finding = Probe::run(&mut device);
    assert_eq!(finding.verdict, Verdict::Unknown);
    assert!(finding.reason.contains("raw socket"), "{}", finding.reason);
    assert!(finding.reason.contains("unavailable"), "{}", finding.reason);
    assert!(finding.checks.iter().all(|check| {
        matches!(&check.outcome, pod_sidecar::probe::CheckOutcome::Unavailable(reason) if reason.contains("raw socket"))
    }));
}

#[test]
fn the_raw_socket_device_reports_a_named_skip_when_the_host_refuses_it() {
    match raw_capability() {
        Ok(()) => {
            // The host allows it; another test gives it a live target.
        }
        Err(refusal) => {
            let mut device = Unavailable::new(refusal.reason());
            let finding = Probe::run(&mut device);
            assert_eq!(finding.verdict, Verdict::Unknown);
            assert!(finding.reason.contains("raw socket"), "{}", finding.reason);
            eprintln!("skipping, {}", finding.reason);
        }
    }
}
