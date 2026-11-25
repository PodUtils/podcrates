//! The proof of the crate against swtpm (libtpms), a TPM in software that
//! this crate did not write (call C151): the sequences of `flow` over
//! `transport::Socket` with `Framing::Raw` on a Unix socket of the test.
//! A key sealed to PCR 7 signs and `p256` verifies it; after
//! `TPM2_PCR_Extend` it is refused; a key under a policy key signs again
//! after a signed policy of the new state; a copy does not load in a
//! second swtpm; the digests of the crate are swtpm's; no handle stays.
//! Each test skips with its reason on a host with no swtpm
//! (`tests/sim/mod.rs`).
#![cfg(all(feature = "device", unix))]

mod common;
mod sim;

use common::proof;
use pod_tpm::transport::Limits;

/// swtpm for the test `tag`, or a return when this host has none.
macro_rules! swtpm {
    ($tag:expr) => {
        match sim::swtpm($tag) {
            Some(started) => started,
            None => return,
        }
    };
}

#[test]
fn a_sealed_key_signs_while_the_boot_state_is_the_recorded_one() {
    let (sim, mut tpm) = swtpm!("seal");
    proof::sealed_to_pcr7(&mut tpm, "swtpm");
    drop(sim);
}

#[test]
fn after_a_signed_policy_for_the_new_state_the_key_signs_again() {
    let (_sim, mut tpm) = swtpm!("signed");
    proof::signed_policies(&mut tpm, "swtpm");
}

#[test]
fn a_copy_of_the_key_file_does_not_load_in_another_tpm() {
    let (_first, mut first) = swtpm!("copy-a");
    let (_second, mut second) = swtpm!("copy-b");
    proof::copy_elsewhere(&mut first, &mut second, "swtpm");
}

#[test]
fn a_signer_that_ignores_the_policy_is_refused() {
    let (_sim, mut tpm) = swtpm!("password");
    proof::signer_ignores_policy(&mut tpm, "swtpm");
}

#[test]
fn a_ticket_of_nothing_is_refused_outside_a_trial() {
    let (_sim, mut tpm) = swtpm!("ticket");
    proof::null_ticket(&mut tpm, "swtpm");
}

#[test]
fn the_capabilities_name_the_tpm() {
    let (sim, mut tpm) = swtpm!("caps");
    proof::capabilities(&mut tpm, "swtpm");
    println!("swtpm: {}", sim.name);
}

/// The time of `TPM2_CreatePrimary` of the storage primary against the
/// default time limit of a command of a transport (`Limits::default`).
#[test]
fn the_storage_primary_fits_the_time_limit() {
    let (_sim, mut tpm) = swtpm!("primary");
    proof::primary_fits(&mut tpm, Limits::default().command, "swtpm");
}
