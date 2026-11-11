//! The fuzz target `oci_challenge`: `podcrates_fuzz::targets::oci_challenge` says what it checks.
#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| podcrates_fuzz::targets::oci_challenge(data));
