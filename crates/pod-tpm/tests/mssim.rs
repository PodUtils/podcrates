//! The proof of the crate against the simulator of the reference
//! implementation (`microsoft/ms-tpm-20-ref`), a second TPM in software
//! that this crate did not write (call C151): an error that is common to
//! libtpms and to the code of this crate shows here. The sequences of
//! `flow` go over `transport::Socket` with `Framing::Mssim` on 127.0.0.1.
//! Each test skips with its reason on a host with no simulator
//! (`tests/sim/mod.rs`).
#![cfg(all(feature = "device", unix))]

mod common;
mod sim;

use common::proof;
use pod_tpm::transport::Limits;

/// The simulator for the test `tag`, or a return when this host has none.
macro_rules! mssim {
    ($tag:expr) => {{
        pod_testkit::need_loopback!();
        match sim::mssim($tag) {
            Some(started) => started,
            None => return,
        }
    }};
}

const NAME: &str = "ms-tpm-20-ref";

#[test]
fn a_sealed_key_signs_while_the_boot_state_is_the_recorded_one() {
    let (_sim, mut tpm) = mssim!("ref-seal");
    proof::sealed_to_pcr7(&mut tpm, NAME);
}

#[test]
fn after_a_signed_policy_for_the_new_state_the_key_signs_again() {
    let (_sim, mut tpm) = mssim!("ref-signed");
    proof::signed_policies(&mut tpm, NAME);
}

#[test]
fn a_copy_of_the_key_file_does_not_load_in_another_tpm() {
    let (_first, mut first) = mssim!("ref-copy-a");
    let (_second, mut second) = mssim!("ref-copy-b");
    proof::copy_elsewhere(&mut first, &mut second, NAME);
}

#[test]
fn a_signer_that_ignores_the_policy_is_refused() {
    let (_sim, mut tpm) = mssim!("ref-password");
    proof::signer_ignores_policy(&mut tpm, NAME);
}

#[test]
fn a_ticket_of_nothing_is_refused_outside_a_trial() {
    let (_sim, mut tpm) = mssim!("ref-ticket");
    proof::null_ticket(&mut tpm, NAME);
}

#[test]
fn the_capabilities_name_the_tpm() {
    let (_sim, mut tpm) = mssim!("ref-caps");
    proof::capabilities(&mut tpm, NAME);
}

#[test]
fn the_storage_primary_fits_the_time_limit() {
    let (_sim, mut tpm) = mssim!("ref-primary");
    proof::primary_fits(&mut tpm, Limits::default().command, NAME);
}
