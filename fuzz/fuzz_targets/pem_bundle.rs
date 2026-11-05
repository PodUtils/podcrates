//! The fuzz target `pem_bundle`: `podcrates_fuzz::targets::pem_bundle` says what it checks.
#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| podcrates_fuzz::targets::pem_bundle(data));
